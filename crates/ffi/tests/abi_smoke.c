/* An actual C caller for the exported Rust cdylib, on a disposable path. */
#include "portaweave.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
int main(int argc, char **argv) {
    uint64_t session = 0, file = 0, handle = 0, found = 0;
    size_t written = 0;
    PwFileStat stat = {0};
    PwHealth health = {0};
    const char *name = "工程.c";
    const char *renamed = "new.c";
    unsigned char content[] = "native linker and ABI";
    unsigned char output[64] = {0};
    if (argc != 2) {
        fprintf(stderr, "expected new disposable test path\n");
        return 2;
    }
    assert(pw_volume_create(argv[1], UINT64_C(67108864), &session) == PW_OK);
    assert(session != 0);
    assert(pw_create_file(session, PW_ROOT_ID, (const uint8_t*)name, strlen(name), &file) == PW_OK);
    assert(pw_open_file(session, file, &handle) == PW_OK);
    assert(pw_write(session, handle, 0, content, sizeof(content), &written) == PW_OK);
    assert(written == sizeof(content));
    assert(pw_read(session, handle, 0, output, sizeof(output), &written) == PW_OK);
    assert(written == sizeof(content));
    assert(memcmp(output, content, sizeof(content)) == 0);
    assert(pw_close_file(session, handle) == PW_OK);
    assert(pw_rename(session, PW_ROOT_ID, (const uint8_t*)name, strlen(name),
                     PW_ROOT_ID, (const uint8_t*)renamed, strlen(renamed), 0) == PW_OK);
    assert(pw_lookup(session, PW_ROOT_ID, (const uint8_t*)renamed, strlen(renamed), &found) == PW_OK);
    assert(found == file);
    assert(pw_stat(session, found, &stat) == PW_OK && stat.size == sizeof(content));
    assert(pw_health(session, &health) == PW_OK && health.write_protected == 0);
    assert(pw_volume_close(session) == PW_OK);
    assert(pw_stat(session, found, &stat) == PW_BAD_HANDLE);
    assert(pw_volume_open(argv[1], &session) == PW_OK);
    assert(pw_lookup(session, PW_ROOT_ID, (const uint8_t*)renamed, strlen(renamed), &found) == PW_OK);
    assert(pw_volume_close(session) == PW_OK);
    puts("PASS C caller -> Rust cdylib -> SQLite + segments -> reopen; not mounted");
    return 0;
}
