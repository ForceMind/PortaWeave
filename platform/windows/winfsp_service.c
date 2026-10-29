/*
 * Experimental WinFsp service ENTRYPOINT, not a complete filesystem.
 * --self-test verifies callbacks without driver or mount.
 * --lab-mount R: creates a NEW disposable %TEMP% container, loads an already
 * installed WinFsp runtime, and runs a real kernel dispatcher until Ctrl+C.
 * Opens denied, Flush unsupported: never store files or treat as usable disk.
 */
#define PW_WINFSP_NO_TEST 1
#include "winfsp_contract.c"
#include <wchar.h>
#include <wctype.h>
#include <stdlib.h>
#include <strsafe.h>
#include "../common/dispatcher_lifecycle_contract.h"

typedef NTSTATUS (*PW_FS_CREATE)(PWSTR,
    const FSP_FSCTL_VOLUME_PARAMS *, const FSP_FILE_SYSTEM_INTERFACE *,
    FSP_FILE_SYSTEM **);
typedef NTSTATUS (*PW_FS_MOUNT)(FSP_FILE_SYSTEM *, PWSTR);
typedef NTSTATUS (*PW_FS_START)(FSP_FILE_SYSTEM *, ULONG);
typedef VOID (*PW_FS_STOP)(FSP_FILE_SYSTEM *);
typedef VOID (*PW_FS_DELETE)(FSP_FILE_SYSTEM *);
typedef struct {
    HMODULE lib;
    PW_FS_CREATE create;
    PW_FS_MOUNT mount;
    PW_FS_START start;
    PW_FS_STOP stop;
    PW_FS_DELETE destroy;
} PW_API;
static HANDLE StopSignal = NULL;
static SRWLOCK StopLock = SRWLOCK_INIT;
static volatile LONG DispatcherAbnormal = 0;
static BOOL PwSignalStop(void)
{
    BOOL result=FALSE;
    AcquireSRWLockShared(&StopLock);
    if (StopSignal) result=SetEvent(StopSignal);
    ReleaseSRWLockShared(&StopLock);
    return result;
}
static BOOL WINAPI PwConsoleCtrl(DWORD event)
{
    /* Close/logoff/shutdown can terminate immediately when this handler
     * returns. They are not a graceful asynchronous Ctrl+C path. */
    if (event!=CTRL_C_EVENT && event!=CTRL_BREAK_EVENT) return FALSE;
    return PwSignalStop();
}
static VOID PwDispatcherStopped(FSP_FILE_SYSTEM *fs, BOOLEAN normally)
{
    (void)fs;
    if (!normally) InterlockedExchange(&DispatcherAbnormal,1);
    /* Never recursively stop/delete from the dispatcher callback. */
    (void)PwSignalStop();
}
static VOID PwServiceInterfaceInit(FSP_FILE_SYSTEM_INTERFACE *callbacks)
{
    PwInterfaceInit(callbacks);
    callbacks->DispatcherStopped=PwDispatcherStopped;
}
static BOOL PwCloseStopSignal(HANDLE expected)
{
    BOOL ok=TRUE;
    /* Wait for in-flight signal users before retiring the handle. */
    AcquireSRWLockExclusive(&StopLock);
    if (expected && StopSignal==expected) { ok=CloseHandle(StopSignal); StopSignal=NULL; }
    else if (expected) ok=FALSE;
    ReleaseSRWLockExclusive(&StopLock);
    return ok;
}
typedef struct {
    PW_API *api;
    FSP_FILE_SYSTEM *fs;
    BOOL registered;
    HANDLE signal;
} PW_DISPATCH_RUN;
static int PwRunPrepare(void *ctx)
{
    PW_DISPATCH_RUN *run=ctx;
    HANDLE signal=CreateEventW(NULL,TRUE,FALSE,NULL);
    if (!signal) return 0;
    AcquireSRWLockExclusive(&StopLock);
    if (StopSignal) {
        ReleaseSRWLockExclusive(&StopLock);
        CloseHandle(signal); return 0;
    }
    StopSignal=signal;
    run->signal=signal;
    ReleaseSRWLockExclusive(&StopLock);
    InterlockedExchange(&DispatcherAbnormal,0);
    run->registered=SetConsoleCtrlHandler(PwConsoleCtrl,TRUE);
    return run->registered;
}
static int PwRunStart(void *ctx)
{
    PW_DISPATCH_RUN *run=ctx;
    NTSTATUS status=run->api->start(run->fs,1);
    if (!NT_SUCCESS(status)) fwprintf(stderr,L"FspStart NTSTATUS 0x%08lX\n",(ULONG)status);
    return NT_SUCCESS(status);
}
static int PwRunWait(void *ctx)
{
    PW_DISPATCH_RUN *run=ctx;
    DWORD result;
    /* Only the serial lifecycle thread retires StopSignal after this wait. */
    result=WaitForSingleObject(run->signal,INFINITE);
    if (result!=WAIT_OBJECT_0) {
        fwprintf(stderr,L"Dispatcher wait failed: result=%lu error=%lu\n",result,GetLastError());
        return 0;
    }
    return InterlockedCompareExchange(&DispatcherAbnormal,0,0)==0;
}
static void PwRunStop(void *ctx)
{
    PW_DISPATCH_RUN *run=ctx;
    run->api->stop(run->fs);
}
static int PwRunRelease(void *ctx)
{
    PW_DISPATCH_RUN *run=ctx;
    BOOL ok=TRUE;
    if (run->registered) {
        ok=SetConsoleCtrlHandler(PwConsoleCtrl,FALSE);
        run->registered=FALSE;
    }
    if (!PwCloseStopSignal(run->signal)) ok=FALSE;
    run->signal=NULL;
    return ok;
}
static const PW_DISPATCHER_OPS PwRunOps={PwRunPrepare,PwRunStart,PwRunWait,PwRunStop,PwRunRelease};
static unsigned PwRunService(PW_DISPATCH_RUN *run)
{
    unsigned result=PwDispatcherRun(&PwRunOps,run);
    /* An abnormal stop may race a console wake-up after wait inspected it.
     * Recheck after stop has returned; normally=TRUE must never clear it. */
    if (InterlockedCompareExchange(&DispatcherAbnormal,0,0)!=0)
        result |= PW_LIFE_WAIT;
    return result;
}
static VOID PwUnload(PW_API *api) {
    if (api->lib) FreeLibrary(api->lib);
    ZeroMemory(api, sizeof(*api));
}
static BOOL PwLoadInstalled(PW_API *api)
{
    WCHAR base[MAX_PATH], path[MAX_PATH];
    const WCHAR *envs[] = {L"ProgramFiles(x86)", L"ProgramFiles"};
    const WCHAR *names[] = {L"winfsp-x64.dll", L"winfsp_x64.dll"};
    UINT i, j;
    ZeroMemory(api, sizeof(*api));
    for (i=0; i<ARRAYSIZE(envs) && !api->lib; ++i) {
        DWORD n = GetEnvironmentVariableW(envs[i], base, ARRAYSIZE(base));
        if (!n || n>=ARRAYSIZE(base)) continue;
        for (j=0; j<ARRAYSIZE(names); ++j) {
            if (FAILED(StringCchPrintfW(path, ARRAYSIZE(path),
                L"%s\\WinFsp\\bin\\%s",base,names[j]))) continue;
            if (GetFileAttributesW(path)==INVALID_FILE_ATTRIBUTES) continue;
            api->lib = LoadLibraryExW(path,NULL,
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR|LOAD_LIBRARY_SEARCH_DEFAULT_DIRS);
            if (api->lib) break;
        }
    }
    if (!api->lib) return FALSE;
#define GET(member,sym,kind) do { \
    api->member=(kind)GetProcAddress(api->lib,#sym); \
    if (!api->member) { PwUnload(api);return FALSE; } \
} while (0)
    GET(create,FspFileSystemCreate,PW_FS_CREATE);
    GET(mount,FspFileSystemSetMountPoint,PW_FS_MOUNT);
    GET(start,FspFileSystemStartDispatcher,PW_FS_START);
    GET(stop,FspFileSystemStopDispatcher,PW_FS_STOP);
    GET(destroy,FspFileSystemDelete,PW_FS_DELETE);
#undef GET
    return TRUE;
}
/* Real Win32 event/handler/SRW path, fake dispatcher only. No runtime/driver. */
static unsigned PwTestStarts, PwTestStops, PwTestAbnormal;
static NTSTATUS PwTestStart(FSP_FILE_SYSTEM *fs, ULONG threads)
{
    (void)threads;
    ++PwTestStarts;
    if (!StopSignal || PwConsoleCtrl(CTRL_CLOSE_EVENT) ||
        PwConsoleCtrl(CTRL_LOGOFF_EVENT) || PwConsoleCtrl(CTRL_SHUTDOWN_EVENT))
        return STATUS_INVALID_PARAMETER;
    if (PwTestAbnormal==1) PwDispatcherStopped(fs,FALSE);
    else if (!PwConsoleCtrl(CTRL_BREAK_EVENT)) return STATUS_INVALID_PARAMETER;
    return STATUS_SUCCESS;
}
static VOID PwTestStop(FSP_FILE_SYSTEM *fs)
{
    ++PwTestStops;
    PwDispatcherStopped(fs,PwTestAbnormal==2 ? FALSE : TRUE);
}
static int PwNativeLifecycleTest(void)
{
    PW_API api;
    FSP_FILE_SYSTEM fs;
    PW_DISPATCH_RUN run, foreign;
    unsigned result, abnormal;
    ZeroMemory(&api,sizeof(api)); ZeroMemory(&fs,sizeof(fs));
    api.start=PwTestStart; api.stop=PwTestStop;
    for (abnormal=0;abnormal<3;++abnormal) {
        ZeroMemory(&run,sizeof(run)); run.api=&api; run.fs=&fs;
        PwTestAbnormal=abnormal;
        PwTestStarts=PwTestStops=0;
        result=PwRunService(&run);
        if (result!=(unsigned)(abnormal ? PW_LIFE_WAIT : 0) || PwTestStarts!=1 ||
            PwTestStops!=1 || StopSignal || run.signal || run.registered ||
            PwConsoleCtrl(CTRL_C_EVENT)) return 1;
    }
    /* A refused second owner must not retire the first owner's signal. */
    ZeroMemory(&run,sizeof(run)); ZeroMemory(&foreign,sizeof(foreign));
    if (!PwRunPrepare(&run)) return 1;
    if (PwRunPrepare(&foreign) || !PwRunRelease(&foreign) ||
        !PwConsoleCtrl(CTRL_C_EVENT) ||
        WaitForSingleObject(run.signal,0)!=WAIT_OBJECT_0 || !PwRunRelease(&run)) return 1;
    if (StopSignal || PwConsoleCtrl(CTRL_C_EVENT)) return 1;
    puts("WINFSP_NATIVE_CONTROL_LIFECYCLE_OK (real Win32 events/handler, fake dispatcher; no driver/mount)");
    return 0;
}
static int PwSelfTest(void)
{
    FSP_FILE_SYSTEM_INTERFACE callbacks;
    FSP_FILE_SYSTEM mock;
    FSP_FSCTL_FILE_INFO dummy;
    PW_PROBE_VOLUME owner;
    FSP_FSCTL_VOLUME_PARAMS params;
    ZeroMemory(&mock,sizeof(mock));
    ZeroMemory(&dummy,sizeof(dummy));
    ZeroMemory(&owner,sizeof(owner));
    mock.UserContext=&owner;
    PwServiceInterfaceInit(&callbacks);
    PwVolumeParamsInit(&params);
    if (pw_abi_version()!=1 || callbacks.GetVolumeInfo!=PwGetVolumeInfo ||
        callbacks.Read!=PwRead || callbacks.Open!=PwOpen ||
        callbacks.DispatcherStopped!=PwDispatcherStopped ||
        callbacks.GetSecurityByName!=PwGetSecurityByName ||
        callbacks.Flush!=PwFlush || callbacks.Create!=NULL ||
        callbacks.Write!=PwWrite || callbacks.GetFileInfo!=PwGetFileInfo ||
        callbacks.SetFileSize!=PwSetFileSize || callbacks.SetBasicInfo!=PwSetBasicInfo ||
        callbacks.ReadDirectory!=PwReadDirectory || !params.DirectoryMarkerAsNextOffset ||
        params.PersistentAcls || params.HardLinks ||
        callbacks.GetSecurityByName(&mock,L"\\",NULL,NULL,NULL)!=STATUS_ACCESS_DENIED ||
        callbacks.Open(&mock,L"\\",0,0,NULL,&dummy)!=STATUS_ACCESS_DENIED ||
        callbacks.Flush(&mock,NULL,NULL)!=STATUS_NOT_SUPPORTED) {
        fwprintf(stderr,L"FAIL: unsafe callbacks enabled\n");
        return 1;
    }
    if (PwLifecycleContract() || PwNativeLifecycleTest()) return 1;
    wprintf(L"WINFSP_SERVICE_SELFTEST_OK (no driver/mount)\n");
    return 0;
}
static int PwLab(WCHAR *letter)
{
    PW_API api;
    FSP_FILE_SYSTEM *fs=NULL;
    FSP_FILE_SYSTEM_INTERFACE callbacks;
    FSP_FSCTL_VOLUME_PARAMS options;
    PW_PROBE_VOLUME owner;
    WCHAR tmp[MAX_PATH],root[MAX_PATH];
    char utf8[MAX_PATH*4];
    DWORD mask;
    NTSTATUS status;
    int rc=1, size;
    if (wcslen(letter)!=2 || letter[1]!=L':' ||
        towupper(letter[0])<L'R' || towupper(letter[0])>L'Z')
        return 2;
    letter[0]=towupper(letter[0]);
    mask=GetLogicalDrives();
    if (!mask || (mask&(1u<<(letter[0]-L'A')))) {
        fwprintf(stderr,L"Target drive letter is already used\n");
        return 2;
    }
    if (!PwLoadInstalled(&api)) {
        fwprintf(stderr,L"BLOCKED: WinFsp DLL unavailable in Program Files. No installation performed.\n");
        return 3;
    }
    size=(int)GetTempPathW(ARRAYSIZE(tmp),tmp);
    if (size<=0 || size>=(int)ARRAYSIZE(tmp) ||
        FAILED(StringCchPrintfW(root,ARRAYSIZE(root),
            L"%sPortaWeave-Lab-%lu-%lu.pwvol",tmp,
            GetCurrentProcessId(),GetTickCount())) ||
        GetFileAttributesW(root)!=INVALID_FILE_ATTRIBUTES) {
        fwprintf(stderr,L"Could not construct a new disposable lab path\n");
        goto end;
    }
    size=WideCharToMultiByte(CP_UTF8,WC_ERR_INVALID_CHARS,
        root,-1,utf8,ARRAYSIZE(utf8),NULL,NULL);
    if (size<=0) goto end;
    ZeroMemory(&owner,sizeof(owner));
    if (PW_OK!=pw_volume_create((const uint8_t *)utf8,strlen(utf8),
        UINT64_C(134217728),&owner.volume)) {
        fwprintf(stderr,L"Could not create new lab container\n");
        goto end;
    }
    PwServiceInterfaceInit(&callbacks);
    PwVolumeParamsInit(&options);
    status=api.create(L"" FSP_FSCTL_DISK_DEVICE_NAME,&options,&callbacks,&fs);
    if (!NT_SUCCESS(status)) {
        fwprintf(stderr,L"FspCreate NTSTATUS 0x%08lX\n",(ULONG)status);
        goto release;
    }
    fs->UserContext=&owner;
    status=api.mount(fs,letter);
    if (!NT_SUCCESS(status)) {
        fwprintf(stderr,L"FspMount NTSTATUS 0x%08lX\n",(ULONG)status);
        goto release;
    }
    {
        PW_DISPATCH_RUN run={&api,fs,FALSE,NULL};
        unsigned result;
        wprintf(L"LAB ONLY: attempting dispatcher on %s. Backing: %s\n"
                L"All opens DENIED. Ctrl+C/Break requests stop; window close is NOT safe eject.\n",letter,root);
        result=PwRunService(&run);
        if (result) fwprintf(stderr,L"Dispatcher lifecycle failed mask=0x%X\n",result);
        rc=result ? 1 : 0;
    }
release:
    if (fs) api.destroy(fs);
    if (pw_volume_close(owner.volume)!=PW_OK) rc=1;
end:
    PwUnload(&api);
    return rc;
}
int wmain(int argc,wchar_t **argv)
{
    if (argc==2 && !wcscmp(argv[1],L"--self-test")) return PwSelfTest();
    if (argc==3 && !wcscmp(argv[1],L"--lab-mount")) return PwLab(argv[2]);
    fwprintf(stderr,L"PortaWeave WinFsp lifecycle laboratory, not a usable disk\n"
        L"--self-test | --lab-mount R: (requires installed WinFsp)\n");
    return 2;
}
