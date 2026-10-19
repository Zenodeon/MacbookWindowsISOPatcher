#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Resolve the exact present instance on this Mac; do not reuse another Mac's instance ID. */
int main(void) {
    const char *prefix = "PCI\\VEN_8086&DEV_1927&SUBSYS_015D106B";
    ULONG size = 0;
    char *ids, *p;
    if (CM_Get_Device_ID_List_SizeA(&size, NULL, CM_GETIDLIST_FILTER_PRESENT) != CR_SUCCESS || size < 2)
        return 1;
    ids = (char *)calloc(size, 1);
    if (!ids) return 1;
    if (CM_Get_Device_ID_ListA(NULL, ids, size, CM_GETIDLIST_FILTER_PRESENT) != CR_SUCCESS) {
        free(ids); return 1;
    }
    for (p = ids; *p; p += strlen(p) + 1) {
        if (_strnicmp(p, prefix, strlen(prefix)) == 0) {
            puts(p); free(ids); return 0;
        }
    }
    free(ids);
    return 1;
}
