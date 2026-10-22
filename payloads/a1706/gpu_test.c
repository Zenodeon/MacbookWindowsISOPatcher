/* Runs the production enumeration loop with deterministic API responses. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <assert.h>
#include <string.h>
static int scenario,calls,waits;
static const char one[]="PCI\\VEN_8086&DEV_1927&SUBSYS_015D106B&REV_0A\\3&test&0&10\0";
static CONFIGRET WINAPI mock_size(PULONG size,PCSTR filter,ULONG flags) {
    (void)filter;(void)flags;++calls;*size=512;
    return scenario==3?CR_FAILURE:CR_SUCCESS;
}
static CONFIGRET WINAPI mock_list(PCSTR filter,PCHAR buffer,ULONG size,ULONG flags) {
    (void)filter;(void)flags;(void)size;
    if(scenario==2&&calls<3)return CR_BUFFER_SMALL;
    if(scenario==1&&calls<3)return CR_SUCCESS;
    if(scenario==4)return CR_SUCCESS;
    memcpy(buffer,one,sizeof(one));
    if(scenario==5)memcpy(buffer+sizeof(one)-1,one,sizeof(one));
    return CR_SUCCESS;
}
static void WINAPI mock_sleep(DWORD ms){assert(ms==1000);++waits;}
#define CM_Get_Device_ID_List_SizeA mock_size
#define CM_Get_Device_ID_ListA mock_list
#define Sleep mock_sleep
#define main gpu_production_main
#include "a1706gpu.c"
#undef main
int main(void) {
    scenario=0;calls=waits=0;assert(gpu_production_main()==0&&calls==1&&waits==0);
    scenario=1;calls=waits=0;assert(gpu_production_main()==0&&calls==3&&waits==2);
    scenario=2;calls=waits=0;assert(gpu_production_main()==0&&calls==3&&waits==2);
    scenario=3;calls=waits=0;assert(gpu_production_main()==1&&calls==5&&waits==4);
    scenario=4;calls=waits=0;assert(gpu_production_main()==1&&calls==5&&waits==4);
    scenario=5;calls=waits=0;assert(gpu_production_main()==2&&calls==1&&waits==0);
    {char selected[512]={0};assert(matching_ids("PCI\\VEN_8086&DEV_1927&SUBSYS_015D106BBAD\0",selected,sizeof(selected))==0);}
    puts("GPU production-path cases passed");return 0;
}
