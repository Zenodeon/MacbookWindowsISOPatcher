#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Instance IDs belong to a machine, not a model. */
static int matching_ids(const char *ids, char *selected, size_t capacity) {
    const char *prefix = "PCI\\VEN_8086&DEV_1927&SUBSYS_015D106B";
    const char *p;
    int count = 0;
    for (p = ids; *p; p += strlen(p) + 1) {
        size_t n = strlen(prefix);
        if (_strnicmp(p, prefix, n) == 0 && (p[n] == '&' || p[n] == '\\')) {
            ++count;
            if (strlen(p) >= capacity) return -1;
            strcpy_s(selected, capacity, p);
        }
    }
    return count;
}

#ifndef GPU_TEST
int main(void) {
    int attempt;
    for (attempt = 1; attempt <= 5; ++attempt) {
        ULONG size = 0;
        CONFIGRET rc = CM_Get_Device_ID_List_SizeA(&size, NULL, CM_GETIDLIST_FILTER_PRESENT);
        if (rc == CR_SUCCESS && size >= 2 && size <= 1024 * 1024) {
            char *ids = (char *)calloc((size_t)size + 2, 1);
            if (ids) {
                char selected[MAX_DEVICE_ID_LEN] = {0};
                rc = CM_Get_Device_ID_ListA(NULL, ids, size, CM_GETIDLIST_FILTER_PRESENT);
                if (rc == CR_SUCCESS) {
                    int count = matching_ids(ids, selected, sizeof(selected));
                    free(ids);
                    if (count == 1 && selected[0]) {
                        puts(selected);
                        fprintf(stderr, "GPU detected on attempt %d: %s\n", attempt, selected);
                        return 0;
                    }
                    if (count != 0) {
                        fprintf(stderr, "GPU detection refused: ambiguous or invalid devices (%d).\n", count);
                        return 2;
                    }
                    fprintf(stderr, "GPU attempt %d: Iris 550 not present.\n", attempt);
                } else {
                    free(ids);
                    fprintf(stderr, "GPU enumeration attempt %d failed (CONFIGRET %lu).\n", attempt, rc);
                }
            } else fprintf(stderr, "GPU enumeration allocation failed.\n");
        } else fprintf(stderr, "GPU list sizing attempt %d failed (CONFIGRET %lu, size %lu).\n", attempt, rc, size);
        if (attempt < 5) Sleep(1000);
    }
    fprintf(stderr, "No unique present A1706 Iris 550 after five attempts. Setup was not started.\n");
    return 1;
}
#endif
