#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static char last[4096];

static void write_all(const char *text)
{
    char path[96];
    HANDLE h;
    DWORD n;
    int i;
    int len;

    len = lstrlenA(text);
    h = CreateFileA("X:\\a1706-after-setup.txt", FILE_APPEND_DATA,
        FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL, NULL);
    if (h != INVALID_HANDLE_VALUE) {
        WriteFile(h, text, (DWORD)len, &n, NULL);
        WriteFile(h, "\r\n", 2, &n, NULL);
        CloseHandle(h);
    }
    for (i = 'C'; i <= 'Z'; i++) {
        char dir[32];
        if (i == 'X')
            continue;
        wsprintfA(dir, "%c:\\A1706Logs", i);
        if (GetFileAttributesA(dir) == INVALID_FILE_ATTRIBUTES)
            continue;
        wsprintfA(path, "%c:\\A1706Logs\\a1706-after-setup.txt", i);
        h = CreateFileA(path, FILE_APPEND_DATA,
            FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL, NULL);
        if (h == INVALID_HANDLE_VALUE)
            continue;
        WriteFile(h, text, (DWORD)len, &n, NULL);
        WriteFile(h, "\r\n", 2, &n, NULL);
        CloseHandle(h);
    }
}

struct grab {
    char buf[3600];
    int used;
};

static void add_text(struct grab *g, const char *text)
{
    int n;
    n = lstrlenA(text);
    if (n <= 0 || g->used + n + 3 >= (int)sizeof(g->buf))
        return;
    if (g->used) {
        g->buf[g->used++] = ' ';
        g->buf[g->used++] = '|';
        g->buf[g->used++] = ' ';
    }
    lstrcpyA(g->buf + g->used, text);
    g->used += n;
}

static BOOL CALLBACK childproc(HWND hwnd, LPARAM lp)
{
    char text[500];
    if (!IsWindowVisible(hwnd))
        return TRUE;
    if (GetWindowTextA(hwnd, text, 480) > 0)
        add_text((struct grab *)lp, text);
    return TRUE;
}

static BOOL CALLBACK topproc(HWND hwnd, LPARAM lp)
{
    char cls[64];
    char title[240];
    struct grab g;
    (void)lp;
    if (!IsWindowVisible(hwnd))
        return TRUE;
    cls[0] = 0;
    title[0] = 0;
    GetClassNameA(hwnd, cls, 64);
    GetWindowTextA(hwnd, title, 220);
    if (lstrcmpA(cls, "#32770") != 0 && lstrcmpiA(title, "Windows Setup") != 0)
        return TRUE;
    ZeroMemory(&g, sizeof(g));
    if (title[0])
        add_text(&g, title);
    EnumChildWindows(hwnd, childproc, (LPARAM)&g);
    if (g.used <= 0)
        return TRUE;
    if (lstrcmpA(g.buf, last) == 0)
        return TRUE;
    lstrcpynA(last, g.buf, (int)sizeof(last));
    write_all(g.buf);
    return TRUE;
}

static void copy_panther(const char *src, const char *local, const char *name)
{
    char path[96];
    char dir[32];
    int i;
    if (GetFileAttributesA(src) == INVALID_FILE_ATTRIBUTES)
        return;
    CopyFileA(src, local, FALSE);
    for (i = 'C'; i <= 'Z'; i++) {
        if (i == 'X')
            continue;
        wsprintfA(dir, "%c:\\A1706Logs", i);
        if (GetFileAttributesA(dir) == INVALID_FILE_ATTRIBUTES)
            continue;
        wsprintfA(path, "%c:\\A1706Logs\\%s", i, name);
        CopyFileA(src, path, FALSE);
    }
}

int WINAPI WinMain(HINSTANCE inst, HINSTANCE prev, LPSTR cmd, int show)
{
    (void)inst;
    (void)prev;
    (void)cmd;
    (void)show;
    write_all("after-setup log started");
    for (;;) {
        EnumWindows(topproc, 0);
        copy_panther("X:\\Windows\\Panther\\setuperr.log",
            "X:\\a1706-setuperr.log", "a1706-setuperr.log");
        copy_panther("X:\\Windows\\Panther\\setupact.log",
            "X:\\a1706-setupact.log", "a1706-setupact.log");
        Sleep(500);
    }
}
