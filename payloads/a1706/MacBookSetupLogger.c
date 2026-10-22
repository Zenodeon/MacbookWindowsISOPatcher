/* Native WinPE diagnostics. No PowerShell, services, autoruns, or network I/O. */
#define WIN32_LEAN_AND_MEAN
#define _CRT_SECURE_NO_WARNINGS
#include <windows.h>
#include <winioctl.h>
#include <cfgmgr32.h>
#include <objbase.h>
#include <stdio.h>
#include <stdlib.h>
#include <wchar.h>
#include <string.h>

#define CAP (16u * 1024u * 1024u)
#define BUDGET (64ull * 1024ull * 1024ull)
#define MARKER L"MacBookPatcher-Logging.enabled"
#define MAGIC "MacBookWindowsISOPatcher setup logging v1"
#define P 1024
static int profile2017;
static int select_profile(const wchar_t *profile) {
    if(!wcscmp(profile,L"a1706")){profile2017=0;return 1;}
    if(!wcscmp(profile,L"a1706-2017")){profile2017=1;return 1;}
    return 0;
}
static const wchar_t *profile_prefix(void) {return profile2017?L"--profile a1706-2017 ":L"";}
static int profile_arguments(int *argc,wchar_t ***argv) {
    if(*argc>=2&&!wcscmp((*argv)[1],L"--profile")) {
        if(*argc<3||!select_profile((*argv)[2]))return 0;
        *argc-=2;*argv+=2;
    }
    return 1;
}
static void watch_command(const wchar_t *exe,wchar_t *command,size_t capacity) {
    _snwprintf_s(command,capacity,_TRUNCATE,L"\"%s\" %s--watch",exe,profile_prefix());
}
static wchar_t spool[P], session[96], home[P];
static ULONGLONG started;
static unsigned errors, truncated, usb_count;
#ifdef DIAGNOSTICS_TEST
static int fake_usb;
static wchar_t fixture_roots[4][P];
static wchar_t full_root[P];
static wchar_t fixture_windows[P];
#endif
static int root_at(int index, wchar_t *root) {
#ifdef DIAGNOSTICS_TEST
    if(index>=4||!fixture_roots[index][0])return 0;
    wcscpy_s(root,P,fixture_roots[index]);return 1;
#else
    if(index>=26)return 0;
    swprintf_s(root,P,L"%c:\\",'A'+index);return 1;
#endif
}

static void join(wchar_t *out, const wchar_t *a, const wchar_t *b) {
    _snwprintf_s(out, P, _TRUNCATE, L"%s\\%s", a, b);
}
static int ordinary(const wchar_t *p, int directory) {
    DWORD a = GetFileAttributesW(p);
    return a != INVALID_FILE_ATTRIBUTES && !(a & FILE_ATTRIBUTE_REPARSE_POINT)
        && !!(a & FILE_ATTRIBUTE_DIRECTORY) == directory;
}
/* All caller-supplied relatives are constants or generated IDs, never marker contents. */
static int directories(const wchar_t *p) {
    wchar_t buf[P]; size_t i, n = wcslen(p);
    if (n < 3 || n >= P || p[1] != ':' || p[2] != '\\' || wcsstr(p, L"..")) return 0;
    wcscpy_s(buf, P, p);
    for (i = 3; i <= n; ++i) if (!buf[i] || buf[i] == '\\') {
        wchar_t c = buf[i]; buf[i] = 0;
        if (!CreateDirectoryW(buf, NULL) && GetLastError() != ERROR_ALREADY_EXISTS) return 0;
        if (!ordinary(buf, 1)) return 0;
        buf[i] = c;
    }
    return 1;
}
static int safe_file(const wchar_t *p) {
    wchar_t parent[P], *slash; DWORD a;
    wcscpy_s(parent, P, p); slash = wcsrchr(parent, '\\');
    if (!slash || slash <= parent + 2) return 0;
    *slash = 0;
    if (!directories(parent)) return 0;
    a = GetFileAttributesW(p);
    return a == INVALID_FILE_ATTRIBUTES || (!(a & FILE_ATTRIBUTE_REPARSE_POINT) && !(a & FILE_ATTRIBUTE_DIRECTORY));
}
static int bytes(const wchar_t *path, const void *data, DWORD len, int append, DWORD limit) {
    HANDLE h; DWORD done; LARGE_INTEGER size; BY_HANDLE_FILE_INFORMATION info; int ok;
#ifdef DIAGNOSTICS_TEST
    if(full_root[0]&&!wcsncmp(path,full_root,wcslen(full_root))){SetLastError(ERROR_DISK_FULL);return 0;}
#endif
    if (!safe_file(path)) return 0;
    h = CreateFileW(path, GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ, NULL,
        OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT, NULL);
    if (h == INVALID_HANDLE_VALUE) return 0;
    if (!GetFileInformationByHandle(h, &info) || info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) { CloseHandle(h); return 0; }
    if (!GetFileSizeEx(h, &size)) { CloseHandle(h); return 0; }
    if (append && size.QuadPart + len > limit) {
        wchar_t flag[P];const char *notice="Partial collection: a bounded timeline, dialog, command or status log reached its size limit. Earlier retained entries remain available.\r\n";
        CloseHandle(h);++truncated;
        if(home[0]){join(flag,home,L"collection-limits.txt");bytes(flag,notice,(DWORD)strlen(notice),0,512);}
        SetLastError(ERROR_FILE_TOO_LARGE);return 0;
    }
    SetFilePointer(h, 0, NULL, append ? FILE_END : FILE_BEGIN);
    ok = WriteFile(h, data, len, &done, NULL) && done == len;
    if (!append) SetEndOfFile(h);
    FlushFileBuffers(h); CloseHandle(h); return ok;
}
static void stamp(char *s, size_t n) {
    SYSTEMTIME t; GetSystemTime(&t);
    _snprintf_s(s,n,_TRUNCATE,"%04u-%02u-%02uT%02u:%02u:%02u.%03uZ",t.wYear,t.wMonth,t.wDay,t.wHour,t.wMinute,t.wSecond,t.wMilliseconds);
}
static void escaped(const char *in, char *out, size_t n) {
    size_t i=0;
    while (*in && i+7<n) {
        unsigned char c=(unsigned char)*in++;
        if (c=='"' || c=='\\') {out[i++]='\\';out[i++]=(char)c;}
        else if (c<32 || c>=127) {i += (size_t)_snprintf_s(out+i,n-i,_TRUNCATE,"\\u%04x",c);}
        else out[i++]=(char)c;
    }
    out[i]=0;
}
static void event(const char *stage, const char *result) {
    wchar_t path[P]; char s[40],a[2048],b[2048],line[4400]; stamp(s,sizeof(s));
    escaped(stage,a,sizeof(a)); escaped(result,b,sizeof(b));
    _snprintf_s(line,sizeof(line),_TRUNCATE,"{\"utc\":\"%s\",\"tick_ms\":%llu,\"stage\":\"%s\",\"result\":\"%s\"}\r\n",s,GetTickCount64(),a,b);
    join(path,home,L"timeline.jsonl"); bytes(path,line,(DWORD)strlen(line),1,2*1024*1024);
    if(strncmp(stage,"collector-",10)&&strcmp(stage,"setup-screen-observed")) {
        join(path,home,L"last-stage.txt"); bytes(path,stage,(DWORD)strlen(stage),0,4096);
        join(path,home,L"last-result.txt"); bytes(path,result,(DWORD)strlen(result),0,4096);
    }
}
static void status(const wchar_t *src, DWORD code, const char *state) {
    wchar_t p[P]; char path[2048],esc[3000],s[40],line[3400];
    WideCharToMultiByte(CP_UTF8,0,src,-1,path,sizeof(path),NULL,NULL); escaped(path,esc,sizeof(esc)); stamp(s,sizeof(s));
    _snprintf_s(line,sizeof(line),_TRUNCATE,"{\"utc\":\"%s\",\"source\":\"%s\",\"state\":\"%s\",\"win32_error\":%lu}\r\n",s,esc,state,code);
    join(p,home,L"collection-status.jsonl"); bytes(p,line,(DWORD)strlen(line),1,2*1024*1024);
}
static ULONGLONG tree_size(const wchar_t *root) {
    wchar_t pattern[P],p[P]; WIN32_FIND_DATAW d; HANDLE f; ULONGLONG total=0;
    join(pattern,root,L"*"); f=FindFirstFileW(pattern,&d); if(f==INVALID_HANDLE_VALUE)return 0;
    do {
        if(d.cFileName[0]=='.' || d.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)continue;
        join(p,root,d.cFileName);
        total += d.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY ? tree_size(p) : ((ULONGLONG)d.nFileSizeHigh<<32)|d.nFileSizeLow;
    } while(FindNextFileW(f,&d)); FindClose(f); return total;
}
static int tail_copy(const wchar_t *src, const wchar_t *dst, int native_log) {
    HANDLE h; LARGE_INTEGER size,offset; BY_HANDLE_FILE_INFORMATION info; char *buf;
    DWORD want,got; ULONGLONG existing=0,total; WIN32_FILE_ATTRIBUTE_DATA old;
    h=CreateFileW(src,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE|FILE_SHARE_DELETE,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);
    if(h==INVALID_HANDLE_VALUE) {
        DWORD e=GetLastError(); if(native_log)status(src,e,(e==ERROR_FILE_NOT_FOUND||e==ERROR_PATH_NOT_FOUND)?"unavailable":"read-error");
        if(e!=ERROR_FILE_NOT_FOUND&&e!=ERROR_PATH_NOT_FOUND)++errors;
        return 0;
    }
    if(!GetFileInformationByHandle(h,&info) || info.dwFileAttributes&(FILE_ATTRIBUTE_DIRECTORY|FILE_ATTRIBUTE_REPARSE_POINT) || !GetFileSizeEx(h,&size)) {CloseHandle(h);++errors;return 0;}
    if(GetFileAttributesExW(dst,GetFileExInfoStandard,&old)) {
        existing=((ULONGLONG)old.nFileSizeHigh<<32)|old.nFileSizeLow;
        if(existing==(ULONGLONG)(size.QuadPart>CAP?CAP:size.QuadPart) && CompareFileTime(&old.ftLastWriteTime,&info.ftLastWriteTime)==0) {CloseHandle(h);return 1;}
    }
    want=(DWORD)(size.QuadPart>CAP?CAP:size.QuadPart);
    total=tree_size(spool[0]?spool:home);
    if(native_log && total-existing+want>BUDGET-12*1024*1024) {CloseHandle(h);++truncated;status(src,0,"spool-budget-exceeded");return 0;}
    offset.QuadPart=size.QuadPart-want;
    if(!SetFilePointerEx(h,offset,NULL,FILE_BEGIN)) {CloseHandle(h);++errors;return 0;}
    buf=(char*)malloc(want?want:1);
    if(!buf){CloseHandle(h);return 0;}
    if(!ReadFile(h,buf,want,&got,NULL) || got!=want) {free(buf);CloseHandle(h);++errors;return 0;}
    CloseHandle(h);
    if(!bytes(dst,buf,got,0,CAP)) {DWORD e=GetLastError();free(buf);++errors;status(dst,e,"write-error");return 0;}
    free(buf);
    h=CreateFileW(dst,FILE_WRITE_ATTRIBUTES,FILE_SHARE_READ,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);
    if(h!=INVALID_HANDLE_VALUE){SetFileTime(h,NULL,NULL,&info.ftLastWriteTime);CloseHandle(h);}
    if(native_log) {
        if(size.QuadPart>CAP){++truncated;status(src,0,"partial-last-16MB");}
        else status(src,0,"copied");
    }
    return 1;
}
static int usb_root(const wchar_t *root) {
    wchar_t device[8]; HANDLE h; DWORD got;
    STORAGE_PROPERTY_QUERY q; BYTE buffer[4096]; STORAGE_DEVICE_DESCRIPTOR *d=(STORAGE_DEVICE_DESCRIPTOR*)buffer;
#ifdef DIAGNOSTICS_TEST
    if(fake_usb)return ordinary(root,1);
#endif
    if(wcslen(root)!=3 || root[1]!=':' || root[2]!='\\')return 0;
    if(GetDriveTypeW(root)!=DRIVE_FIXED && GetDriveTypeW(root)!=DRIVE_REMOVABLE)return 0;
    if(!ordinary(root,1))return 0;
    swprintf_s(device,8,L"\\\\.\\%c:",root[0]);
    h=CreateFileW(device,0,FILE_SHARE_READ|FILE_SHARE_WRITE,NULL,OPEN_EXISTING,0,NULL);
    if(h==INVALID_HANDLE_VALUE)return 0;
    ZeroMemory(&q,sizeof(q));q.PropertyId=StorageDeviceProperty;q.QueryType=PropertyStandardQuery;
    ZeroMemory(buffer,sizeof(buffer));
    got=0;
    if(!DeviceIoControl(h,IOCTL_STORAGE_QUERY_PROPERTY,&q,sizeof(q),buffer,sizeof(buffer),&got,NULL)) {CloseHandle(h);return 0;}
    CloseHandle(h);return got>=sizeof(STORAGE_DEVICE_DESCRIPTOR)&&d->BusType==BusTypeUsb;
}
static int marked(const wchar_t *root) {
    wchar_t path[P]; HANDLE h; char text[64]={0}; DWORD got=0;
    if(!usb_root(root))return 0;
    join(path,root,MARKER);
    if(!ordinary(path,0))return 0;
    h=CreateFileW(path,GENERIC_READ,FILE_SHARE_READ,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);
    if(h==INVALID_HANDLE_VALUE)return 0;
    ReadFile(h,text,63,&got,NULL);CloseHandle(h);
    return got>=strlen(MAGIC)&&memcmp(text,MAGIC,strlen(MAGIC))==0;
}
static void mirror_tree(const wchar_t *from,const wchar_t *to) {
    wchar_t search[P],a[P],b[P];WIN32_FIND_DATAW d;HANDLE h;
    if(!directories(to)) {++errors;return;}
    join(search,from,L"*");h=FindFirstFileW(search,&d);if(h==INVALID_HANDLE_VALUE)return;
    do {
        if(d.cFileName[0]=='.'||d.dwFileAttributes&FILE_ATTRIBUTE_REPARSE_POINT)continue;
        if(!wcscmp(d.cFileName,L"stop.txt")||!wcscmp(d.cFileName,L"last-stage.txt")||!wcscmp(d.cFileName,L"last-result.txt"))continue;
        join(a,from,d.cFileName);join(b,to,d.cFileName);
        if(d.dwFileAttributes&FILE_ATTRIBUTE_DIRECTORY)mirror_tree(a,b);
        else tail_copy(a,b,0);
    } while(FindNextFileW(h,&d));FindClose(h);
}
static void summary(void);
static void sync_usb(void) {
    wchar_t root[P],p[P],dest[P],targets[26][P];int c;usb_count=0;
    for(c=0;root_at(c,root);++c) {
        if(!marked(root))continue;
        join(p,root,profile2017?L"MacBookPatcher-Logs\\A1706-2017":L"MacBookPatcher-Logs\\A1706");join(dest,p,session);
        if(!directories(dest)){++errors;status(dest,GetLastError(),"usb-unavailable");continue;}
        wcscpy_s(targets[usb_count++],P,dest);
    }
    summary();
    for(c=0;c<(int)usb_count;++c)mirror_tree(home,targets[c]);
}
static void snapshots(void) {
    static const wchar_t *logs[]={L"Windows\\Panther\\setupact.log",L"Windows\\Panther\\setuperr.log",
        L"Windows\\Panther\\UnattendGC\\setupact.log",L"Windows\\Panther\\UnattendGC\\setuperr.log",
        L"Windows\\INF\\setupapi.dev.log",L"Windows\\INF\\setupapi.app.log",
        L"$WINDOWS.~BT\\Sources\\Panther\\setupact.log",L"$WINDOWS.~BT\\Sources\\Panther\\setuperr.log",
        L"$WINDOWS.~BT\\Sources\\Rollback\\setupact.log",L"$WINDOWS.~BT\\Sources\\Rollback\\setuperr.log"};
    wchar_t root[P],vol[P],id[96],src[P],dest[P],rel[P],volume_log[P];unsigned i;int c;
    join(volume_log,home,L"volumes.txt");bytes(volume_log,"",0,0,65536);
    for(c=0;root_at(c,root);++c) {
#ifndef DIAGNOSTICS_TEST
        if(GetDriveTypeW(root)!=DRIVE_FIXED&&GetDriveTypeW(root)!=DRIVE_REMOVABLE&&GetDriveTypeW(root)!=DRIVE_RAMDISK)continue;
#endif
        {ULARGE_INTEGER free_bytes,total,available;wchar_t fs[64]={0};char line[512],fs8[128]={0};
         GetVolumeInformationW(root,NULL,0,NULL,NULL,NULL,fs,64);
         WideCharToMultiByte(CP_UTF8,0,fs,-1,fs8,sizeof(fs8),NULL,NULL);
         if(GetDiskFreeSpaceExW(root,&available,&total,&free_bytes)) {
            _snprintf_s(line,sizeof(line),_TRUNCATE,"Volume slot %d; filesystem %s; total bytes %llu; available bytes %llu\r\n",c,fs8,total.QuadPart,available.QuadPart);
            bytes(volume_log,line,(DWORD)strlen(line),1,65536);
         } else status(root,GetLastError(),"volume-info-error");}
        if(GetVolumeNameForVolumeMountPointW(root,vol,P)) {
            unsigned j,k=0;
            for(j=0;vol[j]&&k<80;++j)if((vol[j]>='0'&&vol[j]<='9')||(vol[j]>='a'&&vol[j]<='z')||(vol[j]>='A'&&vol[j]<='Z')||vol[j]=='-')id[k++]=vol[j];id[k]=0;
        } else swprintf_s(id,96,L"drive-%c",'A'+c);
        for(i=0;i<sizeof(logs)/sizeof(logs[0]);++i) {
            join(src,root,logs[i]);_snwprintf_s(rel,P,_TRUNCATE,L"windows-setup\\%s\\%s",id,logs[i]);join(dest,home,rel);
            tail_copy(src,dest,1);
        }
    }
}
static void hardware(void) {
    wchar_t p[P];char text[8192]={0},name[256]={0},model[256]={0};DWORD n;HKEY k;
    if(RegOpenKeyExA(HKEY_LOCAL_MACHINE,"HARDWARE\\DESCRIPTION\\System\\BIOS",0,KEY_READ,&k)==ERROR_SUCCESS) {
        n=sizeof(name);RegQueryValueExA(k,"SystemManufacturer",NULL,NULL,(BYTE*)name,&n);
        n=sizeof(model);RegQueryValueExA(k,"SystemProductName",NULL,NULL,(BYTE*)model,&n);RegCloseKey(k);
    }
    _snprintf_s(text,sizeof(text),_TRUNCATE,"Detected manufacturer: %s\r\nDetected model: %s\r\nSelected profile: %s\r\nTick at collector start: %llu\r\n",name[0]?name:"unavailable",model[0]?model:"unavailable",profile2017?"A1706 2017 / MacBookPro14,2 / Iris Plus 650":"A1706 / MacBookPro13,2 / Iris 550",started);
    join(p,home,L"hardware.txt");bytes(p,text,(DWORD)strlen(text),0,8192);
    /* Device API avoids parsing localized pnputil output for diagnostics. */
    {ULONG size=0;char *ids,*v;
     if(CM_Get_Device_ID_List_SizeA(&size,NULL,CM_GETIDLIST_FILTER_PRESENT)!=CR_SUCCESS||size>1024*1024)return;
     ids=(char*)calloc(size+2,1);if(!ids)return;
     if(CM_Get_Device_ID_ListA(NULL,ids,size,CM_GETIDLIST_FILTER_PRESENT)==CR_SUCCESS)for(v=ids;*v;v+=strlen(v)+1){
        DEVINST dev;ULONG flags=0,problem=0;char cls[128]={0};ULONG len=sizeof(cls);
        if(_strnicmp(v,"PCI\\",4)||CM_Locate_DevNodeA(&dev,v,CM_LOCATE_DEVNODE_NORMAL)!=CR_SUCCESS)continue;
        CM_Get_DevNode_Registry_PropertyA(dev,CM_DRP_CLASS,NULL,cls,&len,0);if(_stricmp(cls,"Display"))continue;
        CM_Get_DevNode_Status(&flags,&problem,dev,0);
        _snprintf_s(text,sizeof(text),_TRUNCATE,"GPU instance: %s\r\nDevnode status: 0x%08lx; problem: %lu\r\n",v,flags,problem);
        bytes(p,text,(DWORD)strlen(text),1,1024*1024);
        {char keyname[512]={0},keypath[768],version[256]={0},provider[256]={0},inf[256]={0};ULONG keysize=sizeof(keyname);
         if(CM_Get_DevNode_Registry_PropertyA(dev,CM_DRP_DRIVER,NULL,keyname,&keysize,0)==CR_SUCCESS) {
            _snprintf_s(keypath,sizeof(keypath),_TRUNCATE,"SYSTEM\\CurrentControlSet\\Control\\Class\\%s",keyname);
            if(RegOpenKeyExA(HKEY_LOCAL_MACHINE,keypath,0,KEY_READ,&k)==ERROR_SUCCESS) {
                n=sizeof(version);RegQueryValueExA(k,"DriverVersion",NULL,NULL,(BYTE*)version,&n);
                n=sizeof(provider);RegQueryValueExA(k,"ProviderName",NULL,NULL,(BYTE*)provider,&n);
                n=sizeof(inf);RegQueryValueExA(k,"InfPath",NULL,NULL,(BYTE*)inf,&n);RegCloseKey(k);
            }
         }
         _snprintf_s(text,sizeof(text),_TRUNCATE,"Driver version: %s; provider: %s; INF: %s\r\n",version,provider,inf);
         bytes(p,text,(DWORD)strlen(text),1,1024*1024);}
     }free(ids);}
}
static void summary(void) {
    wchar_t p[P];char s[40],text[4096],last[1024]={0},result[1024]={0};HANDLE h;DWORD got;stamp(s,sizeof(s));
    join(p,home,L"last-stage.txt");h=CreateFileW(p,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,NULL,OPEN_EXISTING,0,NULL);
    if(h!=INVALID_HANDLE_VALUE){ReadFile(h,last,sizeof(last)-1,&got,NULL);CloseHandle(h);}
    join(p,home,L"last-result.txt");h=CreateFileW(p,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,NULL,OPEN_EXISTING,0,NULL);
    if(h!=INVALID_HANDLE_VALUE){ReadFile(h,result,sizeof(result)-1,&got,NULL);CloseHandle(h);}
    _snprintf_s(text,sizeof(text),_TRUNCATE,"MacBook ISO Patcher - Windows Setup diagnostics\r\nUpdated UTC: %s (firmware clock may be inaccurate)\r\nLast recorded stage: %s\r\nLast result: %s\r\nCollection errors this process: %u\r\nPartial/truncated snapshots this process: %u\r\nMarked USB destinations found last scan: %u\r\n\r\nThis is a partial observation, not proof that installation completed. Automatic collection stops at reboot. RAM-only logs are lost at reboot. Native logs exceeding 16 MB retain their tail; see collection-status.jsonl.\r\n\r\nAttach this entire session folder and the separately saved build log to a GitHub issue. Describe the Mac model, display connection, failure and last visible screen. Review native logs before sharing; Windows can include paths and account names. Nothing is uploaded automatically.\r\nhttps://github.com/Zenodeon/MacbookWindowsISOPatcher/issues\r\n",s,last,result,errors,truncated,usb_count);
    join(p,home,L"summary.txt");bytes(p,text,(DWORD)strlen(text),0,sizeof(text));
}
static char previous[3600];
static void session_state(int closed, const char *mode) {
    wchar_t p[P];char utc[40],text[512];stamp(utc,sizeof(utc));
    _snprintf_s(text,sizeof(text),_TRUNCATE,"{\"schema_version\":1,\"updated_utc\":\"%s\",\"mode\":\"%s\",\"closed_cleanly\":%s,\"status\":\"%s\",\"installation_success_inferred\":false}\r\n",utc,mode,closed?"true":"false",closed?"collection-finished":"running-or-interrupted");
    join(p,home,L"session-status.json");bytes(p,text,(DWORD)strlen(text),0,sizeof(text));
}
static BOOL CALLBACK child_text(HWND w,LPARAM arg) {
    char *b=(char*)arg,t[400],cls[64];size_t n;
    GetClassNameA(w,cls,sizeof(cls));
    /* Never read edit fields, passwords, account input or product-key controls. */
    if(_stricmp(cls,"Static")&&_stricmp(cls,"Button"))return TRUE;
    if(!IsWindowVisible(w)||!GetWindowTextA(w,t,sizeof(t)))return TRUE;
    n=strlen(b);if(n+strlen(t)+4<3500){strcat_s(b,3600," | ");strcat_s(b,3600,t);}return TRUE;
}
static BOOL CALLBACK dialogs(HWND w,LPARAM ignored) {
    char cls[64],title[256],text[3600];wchar_t p[P];char s[40],line[3800];(void)ignored;
    if(!IsWindowVisible(w))return TRUE;
    GetClassNameA(w,cls,sizeof(cls));GetWindowTextA(w,title,sizeof(title));
    if(strcmp(cls,"#32770")&&_stricmp(title,"Windows Setup"))return TRUE;
    strcpy_s(text,sizeof(text),title);EnumChildWindows(w,child_text,(LPARAM)text);
    if(!text[0]||!strcmp(previous,text))return TRUE;
    strcpy_s(previous,sizeof(previous),text);stamp(s,sizeof(s));_snprintf_s(line,sizeof(line),_TRUNCATE,"%s %s\r\n",s,text);
    join(p,home,L"setup-dialogs.txt");bytes(p,line,(DWORD)strlen(line),1,2*1024*1024);
    event("setup-screen-observed",text);return TRUE;
}
static int context(int fresh) {
    wchar_t win[P],current[P];HANDLE h;DWORD got=0;size_t i;
#ifdef DIAGNOSTICS_TEST
    if(fixture_windows[0])wcscpy_s(win,P,fixture_windows);else
#endif
    GetWindowsDirectoryW(win,P);
    join(spool,win,L"Temp\\MacBookPatcher-SetupLogs");
    if(!directories(spool))return 0;join(current,spool,profile2017?L"current-a1706-2017.txt":L"current.txt");
    if(fresh) {
        GUID id;SYSTEMTIME t;wchar_t g[40];
        if(tree_size(spool)>=BUDGET-12*1024*1024){fputs("Local support spool limit reached; retained sessions were preserved.\n",stderr);return 0;}
        if(CoCreateGuid(&id)!=S_OK)return 0;StringFromGUID2(&id,g,40);GetSystemTime(&t);
        swprintf_s(session,96,L"%04u%02u%02u-%02u%02u%02u-%.36s",t.wYear,t.wMonth,t.wDay,t.wHour,t.wMinute,t.wSecond,g+1);
        if(fresh==1&&!bytes(current,session,(DWORD)(wcslen(session)*sizeof(wchar_t)),0,256))return 0;
    } else {
        h=CreateFileW(current,GENERIC_READ,FILE_SHARE_READ,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);
        if(h==INVALID_HANDLE_VALUE)return 0;ReadFile(h,session,sizeof(session)-sizeof(wchar_t),&got,NULL);CloseHandle(h);
        if(!got||got%2)return 0;session[got/2]=0;
        for(i=0;session[i];++i)if(!((session[i]>='0'&&session[i]<='9')||(session[i]>='a'&&session[i]<='f')||(session[i]>='A'&&session[i]<='F')||session[i]=='-'))return 0;
    }
    join(home,spool,session);started=GetTickCount64();return directories(home);
}
static void manifest(void) {
    wchar_t exe[P],src[P],dest[P],*slash;
    GetModuleFileNameW(NULL,exe,P);slash=wcsrchr(exe,'\\');if(slash)*slash=0;
    join(src,exe,L"build-manifest.json");join(dest,home,L"build-manifest.json");
    if(!tail_copy(src,dest,0))status(src,GetLastError(),"build-manifest-unavailable");
}
static void watch(void) {
    wchar_t stop[P];ULONGLONG next=0,seen=0;join(stop,home,L"stop.txt");
    manifest();hardware();session_state(0,"WinPE");event("collector-started","running; automatic collection ends at reboot");
    for(;;) {
        WIN32_FILE_ATTRIBUTE_DATA data;ULONGLONG value=0;wchar_t timeline[P];
#ifndef DIAGNOSTICS_TEST
        EnumWindows(dialogs,0);
#endif
        join(timeline,home,L"timeline.jsonl");
        if(GetFileAttributesExW(timeline,GetFileExInfoStandard,&data))value=((ULONGLONG)data.nFileSizeHigh<<32)|data.nFileSizeLow;
        if(GetTickCount64()>=next||value!=seen||ordinary(stop,0)) {
            hardware();snapshots();session_state(0,"WinPE");summary();sync_usb();summary();next=GetTickCount64()+5000;seen=value;
        }
        if(ordinary(stop,0))break;Sleep(250);
    }
    event("collector-stopped","Setup returned; no persistent collection");session_state(1,"WinPE");summary();sync_usb();
}
static int prepare_marker(const wchar_t *root) {
        wchar_t p[P];
        if(!usb_root(root)){fputs("Choose the root of a connected USB storage volume. Fixed-disk USB storage is supported.\n",stderr);return 2;}
        join(p,root,MARKER);
        if(GetFileAttributesW(p)!=INVALID_FILE_ATTRIBUTES) {
            if(marked(root))return 0;
            fputs("The marker path already exists with different contents; it was preserved.\n",stderr);return 3;
        }
        /* Root marker is the only file outside the fixed session tree. */
        {HANDLE h=CreateFileW(p,GENERIC_WRITE|DELETE,0,NULL,CREATE_NEW,FILE_FLAG_OPEN_REPARSE_POINT,NULL);DWORD n;
         if(h==INVALID_HANDLE_VALUE){fputs("Could not create the USB marker. Check permissions and write protection, then retry.\n",stderr);return 4;}
         if(!WriteFile(h,MAGIC "\r\n",(DWORD)strlen(MAGIC "\r\n"),&n,NULL)||n!=strlen(MAGIC "\r\n")||!FlushFileBuffers(h)) {
            FILE_DISPOSITION_INFO disposition;disposition.DeleteFile=TRUE;
            SetFileInformationByHandle(h,FileDispositionInfo,&disposition,sizeof(disposition));
            CloseHandle(h);fputs("Could not write the USB marker. Check free space and write protection, then retry.\n",stderr);return 4;
         }CloseHandle(h);return 0;}
}
#ifndef DIAGNOSTICS_TEST
int wmain(int argc,wchar_t **argv) {
    wchar_t exe[P],cmd[P*2],p[P];char a[1024],b[1024];
    if(!profile_arguments(&argc,&argv)){fputs("Unknown or missing logging model profile\n",stderr);return 2;}
    if(argc<2)return 2;
    if(!wcscmp(argv[1],L"--check-usb")&&argc==3) {
        int eligible=usb_root(argv[2]);
        puts(eligible?"USB storage root recognized (read-only check; no marker written).":"Not a supported USB storage root.");
        return eligible?0:1;
    }
    if(!wcscmp(argv[1],L"--prepare-usb")&&argc==3)return prepare_marker(argv[2]);
    if(!wcscmp(argv[1],L"--start")) {
        STARTUPINFOW si;PROCESS_INFORMATION pi;
        if(!context(1))return 1;
        manifest();session_state(0,"WinPE");event("startup","WinPE logging initialized; waiting for marked USB");
        GetModuleFileNameW(NULL,exe,P);watch_command(exe,cmd,P*2);
        ZeroMemory(&si,sizeof(si));si.cb=sizeof(si);
        if(!CreateProcessW(exe,cmd,NULL,NULL,FALSE,CREATE_NO_WINDOW,NULL,NULL,&si,&pi))return 1;
        CloseHandle(pi.hThread);CloseHandle(pi.hProcess);return 0;
    }
    if(!wcscmp(argv[1],L"--collect")) {
        if(!context(2))return 1;event("manual-recovery","collecting available saved Windows Setup logs");
        manifest();session_state(0,"manual-recovery");hardware();snapshots();session_state(1,"manual-recovery");summary();sync_usb();summary();sync_usb();
        wprintf(L"Local support folder: %s\nMarked USB destinations: %u\n",home,usb_count);
        if(!usb_count)puts("No writable marked USB found. Local files remain available. In WinPE they are lost at reboot.");
        return errors?1:0;
    }
    if(!context(0))return 1;
    if(!wcscmp(argv[1],L"--watch")){watch();return 0;}
    if(!wcscmp(argv[1],L"--event")&&argc>=3) {
        WideCharToMultiByte(CP_UTF8,0,argv[2],-1,a,sizeof(a),NULL,NULL);b[0]=0;
        if(argc>=4)WideCharToMultiByte(CP_UTF8,0,argv[3],-1,b,sizeof(b),NULL,NULL);
        event(a,b);return 0;
    }
    if(!wcscmp(argv[1],L"--capture")) {
        wchar_t win[P],src[P],dest[P];HANDLE h;DWORD got;char *raw,*utf8;LARGE_INTEGER size;char s[40],header[80];int count;
        GetWindowsDirectoryW(win,P);join(src,win,L"Temp\\MacBookPatcher-command.txt");join(dest,home,L"command-output.txt");
        if(!ordinary(src,0))return 1;
        h=CreateFileW(src,GENERIC_READ,FILE_SHARE_READ|FILE_SHARE_WRITE,NULL,OPEN_EXISTING,FILE_FLAG_OPEN_REPARSE_POINT,NULL);
        if(h==INVALID_HANDLE_VALUE)return 1;
        if(!GetFileSizeEx(h,&size)||size.QuadPart>4*1024*1024){CloseHandle(h);status(src,0,"command-output-too-large");return 1;}
        raw=(char*)calloc((size_t)size.QuadPart+2,1);if(!raw){CloseHandle(h);return 1;}
        if(!ReadFile(h,raw,(DWORD)size.QuadPart,&got,NULL)){free(raw);CloseHandle(h);return 1;}CloseHandle(h);
        stamp(s,sizeof(s));_snprintf_s(header,sizeof(header),_TRUNCATE,"\r\n--- %s ---\r\n",s);bytes(dest,header,(DWORD)strlen(header),1,4*1024*1024);
        if(got>=2&&((unsigned char)raw[0]==255&&(unsigned char)raw[1]==254||raw[1]==0)) {
            int skip=((unsigned char)raw[0]==255&&(unsigned char)raw[1]==254)?2:0;
            count=WideCharToMultiByte(CP_UTF8,0,(wchar_t*)(raw+skip),(got-skip)/2,NULL,0,NULL,NULL);
            utf8=(char*)malloc(count?count:1);if(utf8){WideCharToMultiByte(CP_UTF8,0,(wchar_t*)(raw+skip),(got-skip)/2,utf8,count,NULL,NULL);bytes(dest,utf8,count,1,4*1024*1024);free(utf8);}
        } else bytes(dest,raw,got,1,4*1024*1024);
        free(raw);return 0;
    }
    if(!wcscmp(argv[1],L"--stop")){join(p,home,L"stop.txt");return bytes(p,"stop",4,0,8)?0:1;}
    return 2;
}
#endif
