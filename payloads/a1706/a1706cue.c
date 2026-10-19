#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <mmsystem.h>

#pragma comment(lib, "winmm.lib")
#pragma comment(lib, "user32.lib")

static int tone(int freq, int ms)
{
    const int rate = 22050;
    int samples;
    int dataBytes;
    int wavBytes;
    unsigned char *wav;
    short *pcm;
    unsigned phase;
    unsigned step;
    int i;

    if (ms < 1)
        ms = 1;
    samples = rate * ms / 1000;
    if (samples < 1)
        samples = 1;
    dataBytes = samples * (int)sizeof(short);
    wavBytes = 44 + dataBytes;
    wav = (unsigned char *)HeapAlloc(GetProcessHeap(), HEAP_ZERO_MEMORY, (SIZE_T)wavBytes);
    if (!wav)
        return 3;

    wav[0] = 'R'; wav[1] = 'I'; wav[2] = 'F'; wav[3] = 'F';
    *(DWORD *)(wav + 4) = (DWORD)(wavBytes - 8);
    wav[8] = 'W'; wav[9] = 'A'; wav[10] = 'V'; wav[11] = 'E';
    wav[12] = 'f'; wav[13] = 'm'; wav[14] = 't'; wav[15] = ' ';
    *(DWORD *)(wav + 16) = 16;
    *(WORD *)(wav + 20) = 1;
    *(WORD *)(wav + 22) = 1;
    *(DWORD *)(wav + 24) = (DWORD)rate;
    *(DWORD *)(wav + 28) = (DWORD)(rate * 2);
    *(WORD *)(wav + 32) = 2;
    *(WORD *)(wav + 34) = 16;
    wav[36] = 'd'; wav[37] = 'a'; wav[38] = 't'; wav[39] = 'a';
    *(DWORD *)(wav + 40) = (DWORD)dataBytes;

    pcm = (short *)(wav + 44);
    phase = 0;
    step = (unsigned)(((unsigned long long)freq << 16) / (unsigned)rate);
    for (i = 0; i < samples; i++) {
        int amp;
        int fade = rate / 80;
        phase += step;
        amp = (int)((phase >> 8) & 255);
        if (amp >= 128)
            amp = 255 - amp;
        amp = (amp - 64) * 180;
        if (i < fade)
            amp = amp * i / fade;
        if (i > samples - fade)
            amp = amp * (samples - i) / fade;
        pcm[i] = (short)amp;
    }

    if (!PlaySoundA((LPCSTR)wav, NULL, SND_MEMORY | SND_SYNC | SND_NODEFAULT)) {
        HeapFree(GetProcessHeap(), 0, wav);
        return 3;
    }
    HeapFree(GetProcessHeap(), 0, wav);
    return 0;
}

static int caps_blink(int blinks)
{
    int n;
    for (n = 0; n < blinks; n++) {
        INPUT in[2];
        UINT sent;
        ZeroMemory(in, sizeof(in));
        in[0].type = INPUT_KEYBOARD;
        in[0].ki.wVk = VK_CAPITAL;
        in[1].type = INPUT_KEYBOARD;
        in[1].ki.wVk = VK_CAPITAL;
        in[1].ki.dwFlags = KEYEVENTF_KEYUP;
        sent = SendInput(2, in, sizeof(INPUT));
        if (sent != 2)
            return 2;
        Sleep(180);
        sent = SendInput(2, in, sizeof(INPUT));
        if (sent != 2)
            return 2;
        Sleep(220);
    }
    return 0;
}

int main(int argc, char **argv)
{
    const char *mode = (argc > 1) ? argv[1] : "speakers";
    int rc = 1;

    if (lstrcmpiA(mode, "speakers") == 0) {
        rc = tone(880, 140);
        if (rc)
            return rc;
        Sleep(90);
        return tone(880, 140);
    }
    if (lstrcmpiA(mode, "keyboard") == 0)
        return caps_blink(3);
    if (lstrcmpiA(mode, "display") == 0) {
        rc = tone(660, 120);
        if (rc)
            return rc;
        Sleep(70);
        rc = tone(880, 120);
        if (rc)
            return rc;
        Sleep(70);
        return tone(1175, 160);
    }
    if (lstrcmpiA(mode, "done") == 0) {
        rc = tone(523, 420);
        if (rc)
            return rc;
        return caps_blink(1);
    }
    return 1;
}
