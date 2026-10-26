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
static BOOL WINAPI PwConsoleCtrl(DWORD event)
{
    if (event == CTRL_C_EVENT || event == CTRL_BREAK_EVENT ||
        event == CTRL_CLOSE_EVENT || event == CTRL_SHUTDOWN_EVENT) {
        if (StopSignal) SetEvent(StopSignal);
        return TRUE;
    }
    return FALSE;
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
    PwInterfaceInit(&callbacks);
    PwVolumeParamsInit(&params);
    if (pw_abi_version()!=1 || callbacks.GetVolumeInfo!=PwGetVolumeInfo ||
        callbacks.Read!=PwRead || callbacks.Open!=PwOpen ||
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
    PwInterfaceInit(&callbacks);
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
    status=api.start(fs,1);
    if (!NT_SUCCESS(status)) {
        fwprintf(stderr,L"FspStart NTSTATUS 0x%08lX\n",(ULONG)status);
        goto release;
    }
    StopSignal=CreateEventW(NULL,TRUE,FALSE,NULL);
    if (!StopSignal || !SetConsoleCtrlHandler(PwConsoleCtrl,TRUE)) {
        api.stop(fs);
        goto release;
    }
    wprintf(L"LAB ONLY: %s mounted with all file opens DENIED. Backing: %s\n"
            L"Press Ctrl+C to stop; never store user data.\n",letter,root);
    (void)WaitForSingleObject(StopSignal,INFINITE);
    (void)SetConsoleCtrlHandler(PwConsoleCtrl,FALSE);
    api.stop(fs);
    rc=0;
release:
    if (StopSignal) {CloseHandle(StopSignal);StopSignal=NULL;}
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
