/* 2017 A1706 / Iris Plus 650 only. Never falls back to a 2016 instance ID. */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cfgmgr32.h>
#include <cfg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int is_2017_id(const char *p) {
    const char *prefix="PCI\\VEN_8086&DEV_5927";size_t n=strlen(prefix);
    return !_strnicmp(p,prefix,n)&&(p[n]=='&'||p[n]=='\\')&&!strpbrk(p,"\"<>|\r\n");
}
static int select_2017_ids(const char *ids,char *selected,size_t cap) {
    const char *p;int count=0;
    for(p=ids;*p;p+=strlen(p)+1)if(is_2017_id(p)) {if(strlen(p)>=cap)return -1;strcpy_s(selected,cap,p);++count;}
    return count;
}
static int discover_2017(void) {
    int attempt;
    for(attempt=1;attempt<=5;++attempt) {
        ULONG size=0;CONFIGRET rc=CM_Get_Device_ID_List_SizeA(&size,NULL,CM_GETIDLIST_FILTER_PRESENT);
        if(rc==CR_SUCCESS&&size>=2&&size<=1024*1024) {
            char *ids=(char*)calloc((size_t)size+2,1);
            if(ids) {
                char selected[MAX_DEVICE_ID_LEN]={0};rc=CM_Get_Device_ID_ListA(NULL,ids,size,CM_GETIDLIST_FILTER_PRESENT);
                if(rc==CR_SUCCESS) {
                    int count=select_2017_ids(ids,selected,sizeof(selected));free(ids);
                    if(count==1) {
                        DEVINST node;char copy[MAX_DEVICE_ID_LEN];char hardware[8192]={0};ULONG bytes=sizeof(hardware)-2,type=0;char *p;
                        strcpy_s(copy,sizeof(copy),selected);
                        fprintf(stderr,"GPU detected on attempt %d: %s\n",attempt,selected);
                        if(CM_Locate_DevNodeA(&node,copy,CM_LOCATE_DEVNODE_NORMAL)==CR_SUCCESS && CM_Get_DevNode_Registry_PropertyA(node,CM_DRP_HARDWAREID,&type,hardware,&bytes,0)==CR_SUCCESS && type==REG_MULTI_SZ && bytes<=sizeof(hardware)-2) {
                            for(p=hardware;*p;p+=strlen(p)+1)fprintf(stderr,"GPU hardware ID: %s\n",p);
                        } else fputs("GPU hardware IDs unavailable\n",stderr);
                        puts(selected);return 0;
                    }
                    if(count!=0){fprintf(stderr,"GPU detection refused: ambiguous or invalid devices (%d)\n",count);return 2;}
                    fprintf(stderr,"GPU attempt %d: Iris Plus 650 not present\n",attempt);
                } else {free(ids);fprintf(stderr,"GPU enumeration attempt %d failed (CONFIGRET %lu)\n",attempt,rc);}
            } else fputs("GPU enumeration allocation failed\n",stderr);
        } else fprintf(stderr,"GPU list sizing attempt %d failed (CONFIGRET %lu)\n",attempt,rc);
        if(attempt<5)Sleep(1000);
    }
    fputs("No unique present Iris Plus 650 after five attempts; Setup was not started\n",stderr);return 1;
}
/* Structured checks bind readiness to the exact discovered device and package. */
static int ready_2017(const char *id) {
    DEVINST node;ULONG flags=0,problem=0,bytes,type=0;char copy[MAX_DEVICE_ID_LEN],service[128]={0},driver[512]={0},path[640],version[128]={0};HKEY key;LONG result;
    strcpy_s(copy,sizeof(copy),id);
    if(CM_Locate_DevNodeA(&node,copy,CM_LOCATE_DEVNODE_NORMAL)!=CR_SUCCESS)return 0;
    if(CM_Get_DevNode_Status(&flags,&problem,node,0)!=CR_SUCCESS)return 0;
    fprintf(stderr,"Readiness device %s: status=0x%08lx problem=%lu\n",id,flags,problem);
    if(!(flags&DN_STARTED)||(flags&DN_HAS_PROBLEM)||problem)return 0;
    bytes=sizeof(service)-1;
    if(CM_Get_DevNode_Registry_PropertyA(node,CM_DRP_SERVICE,&type,service,&bytes,0)!=CR_SUCCESS||type!=REG_SZ||bytes>=sizeof(service)||_stricmp(service,"igfx")) {fprintf(stderr,"Readiness: expected igfx binding; observed %s\n",service);return 0;}
    bytes=sizeof(driver)-1;
    if(CM_Get_DevNode_Registry_PropertyA(node,CM_DRP_DRIVER,&type,driver,&bytes,0)!=CR_SUCCESS||type!=REG_SZ||bytes>=sizeof(driver)||strstr(driver,".."))return 0;
    _snprintf_s(path,sizeof(path),_TRUNCATE,"SYSTEM\\CurrentControlSet\\Control\\Class\\%s",driver);
    if(RegOpenKeyExA(HKEY_LOCAL_MACHINE,path,0,KEY_READ,&key)!=ERROR_SUCCESS)return 0;
    bytes=sizeof(version)-1;result=RegQueryValueExA(key,"DriverVersion",NULL,&type,(LPBYTE)version,&bytes);RegCloseKey(key);
    if(result!=ERROR_SUCCESS||type!=REG_SZ||bytes>=sizeof(version))return 0;
    fprintf(stderr,"Readiness: Intel driver version %s\n",version);
    return !strcmp(version,"27.20.100.9316");
}
static int wait_2017(const char *id) {
    ULONGLONG started=GetTickCount64();
    if(strlen(id)>=MAX_DEVICE_ID_LEN||!is_2017_id(id)){fputs("Invalid 2017 GPU instance; Setup was not started\n",stderr);return 2;}
    for(;;) {
        if(ready_2017(id)){fputs("Iris Plus 650 ready with expected Intel driver\n",stderr);return 0;}
        if(GetTickCount64()-started>=60000)break;
        Sleep(1000);
    }
    fputs("Iris Plus 650 readiness timed out after 60 seconds; Setup was not started\n",stderr);return 1;
}
int main(int argc,char **argv) {
    if(argc==1)return discover_2017();
    if(argc==3&&!strcmp(argv[1],"--wait-ready"))return wait_2017(argv[2]);
    fputs("Invalid 2017 graphics command\n",stderr);return 2;
}
