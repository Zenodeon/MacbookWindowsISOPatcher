/* Not embedded or distributed: exercises production filesystem helpers on fixtures. */
#define DIAGNOSTICS_TEST
#include "MacBookSetupLogger.c"
#include <assert.h>
int wmain(int argc,wchar_t **argv) {
    wchar_t a[P],b[P],usb[P],dest[P],blocked[P];char *large;HANDLE h;DWORD n;
    assert(argc==2&&wcsstr(argv[1],L"macbook-diagnostics-test-"));
    join(home,argv[1],L"spool");assert(directories(home));wcscpy_s(session,96,L"test-session");started=GetTickCount64();
    event("wpeinit-start","begin");event("wpeinit-finish","0");summary();
    join(a,home,L"timeline.jsonl");assert(ordinary(a,0));
    join(usb,argv[1],L"usb");assert(directories(usb));fake_usb=1;
    assert(!marked(usb));assert(prepare_marker(usb)==0);assert(marked(usb));assert(prepare_marker(usb)==0);
    join(a,usb,MARKER);assert(bytes(a,"unrelated",9,0,128));assert(prepare_marker(usb)==3);
    {char preserved[16]={0};h=CreateFileW(a,GENERIC_READ,FILE_SHARE_READ,NULL,OPEN_EXISTING,0,NULL);assert(h!=INVALID_HANDLE_VALUE);ReadFile(h,preserved,15,&n,NULL);CloseHandle(h);assert(!strcmp(preserved,"unrelated"));}
    assert(DeleteFileW(a));assert(prepare_marker(usb)==0);
    fake_usb=0;assert(!marked(usb));fake_usb=1;
    wcscpy_s(fixture_roots[0],P,usb);
    join(fixture_roots[1],argv[1],L"second-usb");assert(directories(fixture_roots[1]));
    join(a,fixture_roots[1],MARKER);assert(bytes(a,MAGIC,(DWORD)strlen(MAGIC),0,128));
    join(fixture_roots[2],argv[1],L"unmarked-usb");assert(directories(fixture_roots[2]));
    /* Simulated Setup source and a live watcher cycle, with no host log reads. */
    join(a,usb,L"Windows\\Panther\\setupact.log");assert(bytes(a,"simulated setup stage",21,0,128));
    join(a,home,L"stop.txt");assert(bytes(a,"stop",4,0,8));
    watch();assert(usb_count==2);
    join(a,fixture_roots[1],L"MacBookPatcher-Logs\\A1706\\test-session\\timeline.jsonl");assert(ordinary(a,0));
    join(a,fixture_roots[2],L"MacBookPatcher-Logs");assert(GetFileAttributesW(a)==INVALID_FILE_ATTRIBUTES);
    /* USB removal and reattachment are re-evaluated for each synchronization. */
    join(a,fixture_roots[1],MARKER);assert(DeleteFileW(a));sync_usb();assert(usb_count==1);
    assert(bytes(a,MAGIC,(DWORD)strlen(MAGIC),0,128));sync_usb();assert(usb_count==2);
    join(a,fixture_roots[1],L"MacBookPatcher-Logs\\A1706\\test-session\\timeline.jsonl");
    assert(SetFileAttributesW(a,FILE_ATTRIBUTE_READONLY));
    {unsigned before=errors;event("driver-load-failed","simulated command result 5");sync_usb();assert(errors>before);}
    assert(SetFileAttributesW(a,FILE_ATTRIBUTE_NORMAL));sync_usb();
    wcscpy_s(full_root,P,fixture_roots[1]);
    {unsigned before=errors;event("setup-copy","simulated progress");sync_usb();assert(errors>before&&usb_count==2);}
    full_root[0]=0;sync_usb();
    /* An inaccessible source is an error, not an unavailable source. */
    join(a,argv[1],L"locked.log");assert(bytes(a,"held",4,0,8));
    h=CreateFileW(a,GENERIC_READ,0,NULL,OPEN_EXISTING,0,NULL);assert(h!=INVALID_HANDLE_VALUE);
    join(b,home,L"locked-copy.log");assert(!tail_copy(a,b,1));CloseHandle(h);
    join(dest,usb,L"MacBookPatcher-Logs\\A1706\\test-session");mirror_tree(home,dest);
    join(a,dest,L"timeline.jsonl");assert(ordinary(a,0));
    /* Full/blocked destination: partial failure does not remove source logs. */
    join(blocked,argv[1],L"blocked");assert(bytes(blocked,"keep",4,0,8));
    {unsigned before=errors;mirror_tree(home,blocked);assert(errors>before);}
    assert(ordinary(a,0));
    /* Tail bound and budget: sources stay outside the local spool. */
    join(a,argv[1],L"large.log");large=(char*)malloc(CAP+128);assert(large);memset(large,'a',CAP+128);large[CAP+127]='Z';
    assert(bytes(a,large,CAP+128,0,CAP+128));free(large);
    join(b,home,L"native\\tail.log");assert(tail_copy(a,b,1));assert(truncated==1);
    h=CreateFileW(b,GENERIC_READ,FILE_SHARE_READ,NULL,OPEN_EXISTING,0,NULL);assert(h!=INVALID_HANDLE_VALUE);assert(GetFileSize(h,NULL)==CAP);SetFilePointer(h,-1,NULL,FILE_END);{char c;assert(ReadFile(h,&c,1,&n,NULL)&&c=='Z');}CloseHandle(h);
    join(b,home,L"native\\second.log");assert(tail_copy(a,b,1));join(b,home,L"native\\third.log");assert(tail_copy(a,b,1));join(b,home,L"native\\fourth.log");assert(!tail_copy(a,b,1));
    assert(tree_size(home)<=BUDGET);
    /* Missing sources and invalid paths remain distinct, and escapes are rejected. */
    join(a,argv[1],L"missing.log");assert(!tail_copy(a,b,1));
    join(a,home,L"..\\escape.txt");assert(!bytes(a,"bad",3,0,8));
    join(a,home,L"link\\escaped.txt");assert(!bytes(a,"bad",3,0,8)); /* Rust creates junction here. */
    /* New session is independent; existing session remains intact. */
    join(dest,usb,L"MacBookPatcher-Logs\\A1706\\second-session");mirror_tree(home,dest);
    join(a,usb,L"MacBookPatcher-Logs\\A1706\\test-session\\timeline.jsonl");assert(ordinary(a,0));
    join(a,dest,L"timeline.jsonl");assert(ordinary(a,0));
    /* Exercise real session creation, reopen, recovery isolation, and unsafe context rejection. */
    join(fixture_windows,argv[1],L"fixture-windows");assert(directories(fixture_windows));
    {wchar_t first[96],second[96];
     assert(context(1));wcscpy_s(first,96,session);assert(context(0)&&!wcscmp(first,session));
     assert(context(2));wcscpy_s(second,96,session);assert(wcscmp(first,second));
     assert(context(0)&&!wcscmp(first,session));
     join(a,spool,L"current.txt");assert(bytes(a,L"..\\escape",18,0,128));assert(!context(0));}
    puts("Native collector fixture cases passed");return 0;
}
