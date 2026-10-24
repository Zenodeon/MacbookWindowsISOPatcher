#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <cfg.h>
#include <stdio.h>
#include <string.h>

/* Query the discovered GPU directly: pnputil status text is localized. */
int main(int argc, char **argv)
{
    const char *prefix = "PCI\\VEN_8086&DEV_1927&SUBSYS_015D106B";
    size_t prefix_len = strlen(prefix);
    DEVINST node;
    ULONG flags = 0, problem = 0, bytes, type = 0;
    char id[MAX_DEVICE_ID_LEN], service[128] = {0};

    if (argc != 2 || strlen(argv[1]) >= sizeof(id) ||
        _strnicmp(argv[1], prefix, prefix_len) ||
        (argv[1][prefix_len] != '&' && argv[1][prefix_len] != '\\'))
        return 2;
    strcpy_s(id, sizeof(id), argv[1]);
    if (CM_Locate_DevNodeA(&node, id, CM_LOCATE_DEVNODE_NORMAL) != CR_SUCCESS ||
        CM_Get_DevNode_Status(&flags, &problem, node, 0) != CR_SUCCESS)
        return 1;
    fprintf(stderr, "GPU status=0x%08lx problem=%lu\n", flags, problem);
    if (!(flags & DN_STARTED) || (flags & DN_HAS_PROBLEM) || problem)
        return 1;
    bytes = sizeof(service) - 1;
    if (CM_Get_DevNode_Registry_PropertyA(node, CM_DRP_SERVICE, &type,
            service, &bytes, 0) != CR_SUCCESS || type != REG_SZ ||
        bytes >= sizeof(service) || _stricmp(service, "igfx"))
        return 1;
    return 0;
}
