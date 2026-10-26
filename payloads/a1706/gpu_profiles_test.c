/* Production-path 2017 GPU tests. No host device is enumerated or restarted. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <cfg.h>
#include <assert.h>
#include <string.h>
static int amd_profile;
static int imac_profile;
static int scenario,calls,waits,ready_calls;static ULONGLONG elapsed;
static const char *one;
static const char intel[]="PCI\\VEN_8086&DEV_1926&SUBSYS_1234106B&REV_06\\test-instance\0";
static CONFIGRET WINAPI mock_size(PULONG size,PCSTR filter,ULONG flags){(void)filter;(void)flags;++calls;*size=512;return scenario==3?CR_FAILURE:CR_SUCCESS;}
static CONFIGRET WINAPI mock_list(PCSTR filter,PCHAR buffer,ULONG size,ULONG flags){(void)filter;(void)flags;(void)size;if(scenario==2&&calls<3)return CR_BUFFER_SMALL;if(scenario==4||(scenario==1&&calls<3))return CR_SUCCESS;memcpy(buffer,one,strlen(one)+2);if(scenario==5)memcpy(buffer+strlen(one)+1,one,strlen(one)+2);return CR_SUCCESS;}
static void WINAPI mock_sleep(DWORD ms){assert(ms==1000);++waits;elapsed+=ms;}
static ULONGLONG WINAPI mock_tick(void){return elapsed;}
static CONFIGRET WINAPI mock_locate(PDEVINST node,DEVINSTID_A id,ULONG flags){(void)id;(void)flags;*node=1;return scenario==7?CR_FAILURE:CR_SUCCESS;}
static CONFIGRET WINAPI mock_status(PULONG flags,PULONG problem,DEVINST node,ULONG unused){(void)node;(void)unused;++ready_calls;*flags=(scenario==11&&ready_calls<3)?0:DN_STARTED;*problem=scenario==8?43:0;return CR_SUCCESS;}
static CONFIGRET WINAPI mock_property(DEVINST node,ULONG property,PULONG type,PVOID buffer,PULONG size,ULONG flags){
 const char *value;size_t n;(void)node;(void)flags;if(scenario==12)return CR_FAILURE;
 value=property==CM_DRP_SERVICE?(scenario==9?"BasicDisplay":(amd_profile?"amdkmdap":"igfx")):property==CM_DRP_DRIVER?"{test-guid}\\0001":one;
 *type=property==CM_DRP_HARDWAREID?REG_MULTI_SZ:REG_SZ;n=strlen(value)+1;if(property==CM_DRP_HARDWAREID)++n;assert(*size>=n);memcpy(buffer,value,n);*size=(ULONG)n;return CR_SUCCESS;
}
static LSTATUS WINAPI mock_open(HKEY root,LPCSTR path,DWORD options,REGSAM access,PHKEY key){(void)root;(void)options;(void)access;assert(strstr(path,"Control\\Class\\{test-guid}\\0001"));*key=(HKEY)1;return ERROR_SUCCESS;}
static LSTATUS WINAPI mock_query(HKEY key,LPCSTR name,LPDWORD reserved,LPDWORD type,LPBYTE data,LPDWORD size){const char *version=scenario==10?"0.0.0.0":(amd_profile?"21.19.128.7":imac_profile?"27.20.100.9316":"21.20.16.5174");(void)key;(void)reserved;assert(!strcmp(name,"DriverVersion"));*type=REG_SZ;assert(*size>strlen(version));strcpy_s((char*)data,*size,version);*size=(DWORD)strlen(version)+1;return ERROR_SUCCESS;}
static LSTATUS WINAPI mock_close(HKEY key){(void)key;return ERROR_SUCCESS;}
#define CM_Get_Device_ID_List_SizeA mock_size
#define CM_Get_Device_ID_ListA mock_list
#define CM_Locate_DevNodeA mock_locate
#define CM_Get_DevNode_Status mock_status
#define CM_Get_DevNode_Registry_PropertyA mock_property
#define RegOpenKeyExA mock_open
#define RegQueryValueExA mock_query
#define RegCloseKey mock_close
#define Sleep mock_sleep
#define GetTickCount64 mock_tick
#define main gpu2017_production_main
#include "MacBookSetupGPU.c"
#undef main
static void reset(int s){scenario=s;calls=waits=ready_calls=0;elapsed=0;}
int main(void){
 const char *profiles[]={"a1708-2016","a1707-2016"};
 const char *devices[]={intel,
 "PCI\\VEN_1002&DEV_67EF&SUBSYS_0160106B&REV_C0\\test\0",
 "PCI\\VEN_1002&DEV_67EF&SUBSYS_0166106B&REV_C7\\test\0",
 "PCI\\VEN_1002&DEV_67EF&SUBSYS_0167106B&REV_EF\\test\0"};
 int d,s;char selected[512];
 assert(!select_profile("a1706"));assert(!select_profile("unknown"));
 for(d=0;d<4;++d){
  assert(select_profile(profiles[d?1:0]));one=devices[d];
  {const char *labels[]={"Intel Iris 540","Radeon Pro 460","Radeon Pro 455","Radeon Pro 450"};assert(!strcmp(gpu_label(one),labels[d]));}
  reset(0);assert(discover_profile()==0&&calls==1&&waits==0);
  reset(1);assert(discover_profile()==0&&calls==3&&waits==2);
  reset(2);assert(discover_profile()==0&&calls==3&&waits==2);
  for(s=3;s<=4;++s){reset(s);assert(discover_profile()==1&&calls==5&&waits==4);}
  reset(5);assert(discover_profile()==2&&calls==1&&waits==0);
  reset(0);assert(wait_profile(one)==0&&ready_calls==1);
  {char *args[]={"gpu","--profile",(char*)profiles[d?1:0],"--wait-ready",(char*)one};reset(0);assert(gpu2017_production_main(5,args)==0&&ready_calls==1);}
  reset(11);assert(wait_profile(one)==0&&ready_calls==3&&waits==2);
  for(s=7;s<=10;++s){reset(s);assert(wait_profile(one)==1&&waits==60&&elapsed==60000);}
  reset(12);assert(wait_profile(one)==1&&waits==60);
  reset(0);assert(wait_profile("PCI\\VEN_8086&DEV_1927\\wrong")==2&&ready_calls==0);
  assert(select_profile_ids("PCI\\VEN_8086&DEV_191B\\hd530\0PCI\\VEN_8086&DEV_1927\\iris550\0",selected,sizeof(selected))==0);
  if(d){char mixed[1024]={0};const char *hd="PCI\\VEN_8086&DEV_191B\\hd530";strcpy_s(mixed,sizeof(mixed),hd);memcpy(mixed+strlen(hd)+1,one,strlen(one)+2);assert(select_profile_ids(mixed,selected,sizeof(selected))==1&&!strcmp(selected,one));}
 }
 {const char imac[]="PCI\\VEN_8086&DEV_5926&SUBSYS_016E106B&REV_06\\imac-fixture\0";
  assert(select_profile("imac18-1-2017"));one=imac;
  reset(0);assert(discover_profile()==0&&calls==1);
  reset(1);assert(discover_profile()==0&&calls==3&&waits==2);
  for(s=3;s<=4;++s){reset(s);assert(discover_profile()==1&&calls==5&&waits==4);}
  reset(5);assert(discover_profile()==2&&calls==1);
  reset(0);assert(wait_profile(one)==0&&ready_calls==1);
  reset(11);assert(wait_profile(one)==0&&ready_calls==3);
  for(s=7;s<=10;++s){reset(s);assert(wait_profile(one)==1&&waits==60);}
  reset(12);assert(wait_profile(one)==1&&waits==60);
  reset(0);assert(wait_profile(intel)==2&&ready_calls==0);
  assert(!strcmp(gpu_label(imac),"Intel Iris Plus 640"));
  assert(select_profile_ids("PCI\\VEN_8086&DEV_5927\\other\0",selected,sizeof(selected))==0);
  {char *args[]={"gpu","--profile","imac18-1-2017","--wait-ready",(char*)one};reset(0);assert(gpu2017_production_main(5,args)==0);}
 }
 puts("2016 profile GPU production-path tests passed");return 0;
}
