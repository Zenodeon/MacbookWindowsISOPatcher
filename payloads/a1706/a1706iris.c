#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <stdlib.h>

static int contains(const char *hay, const char *needle)
{
    const char *h;
    const char *n;
    if (!hay || !needle || !needle[0])
        return 0;
    for (h = hay; *h; h++) {
        for (n = needle; *n && h[n - needle] == *n; n++)
            ;
        if (!*n)
            return 1;
    }
    return 0;
}

int main(void)
{
    HANDLE h;
    DWORD size, got, i, j;
    char *raw;
    char *plain;

    h = CreateFileA("X:\\a1706-cmd-out.txt", GENERIC_READ,
        FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL, NULL);
    if (h == INVALID_HANDLE_VALUE)
        return 1;
    size = GetFileSize(h, NULL);
    if (size == 0 || size == INVALID_FILE_SIZE) {
        CloseHandle(h);
        return 1;
    }
    raw = (char *)malloc((size_t)size);
    plain = (char *)malloc((size_t)size + 1);
    if (!raw || !plain) {
        CloseHandle(h);
        free(raw);
        free(plain);
        return 1;
    }
    if (!ReadFile(h, raw, size, &got, NULL)) {
        CloseHandle(h);
        free(raw);
        free(plain);
        return 1;
    }
    CloseHandle(h);
    j = 0;
    for (i = 0; i < got; i++) {
        if (raw[i] != 0)
            plain[j++] = raw[i];
    }
    plain[j] = 0;
    free(raw);
    if (contains(plain, "Iris") && contains(plain, "Started")) {
        free(plain);
        return 0;
    }
    free(plain);
    return 1;
}
