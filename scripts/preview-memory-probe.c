// Native test-only process accounting. No production process is altered.
#include <libproc.h>
#include <sys/resource.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
int tr_preview_usage(const char *bundle, uint64_t *rss, uint64_t *footprint, uint64_t *count, uint64_t *details) {
    int pids[8192];
    int n=proc_listallpids(pids,sizeof(pids));
    int missing=0;
    size_t prefix=strlen(bundle);
    if (n<=0 || n>=8192) return -1;
    *rss=0; *footprint=0; *count=0;
    for (int i=0;i<n;i++) {
        char path[PROC_PIDPATHINFO_MAXSIZE];
        if (proc_pidpath(pids[i],path,sizeof(path))<=0 || strncmp(path,bundle,prefix)!=0 || path[prefix]!='/') continue;
        struct rusage_info_v4 info={0};
        if (proc_pid_rusage(pids[i],RUSAGE_INFO_V4,(rusage_info_t *)&info)!=0) {
            // Exiting processes may disappear between enumeration and sampling.
            // A still-present matching process must not be silently omitted.
            if (proc_pidpath(pids[i],path,sizeof(path))>0 && strncmp(path,bundle,prefix)==0 && path[prefix]=='/') missing++;
            continue;
        }
        if (*count >= 128) return -2;
        size_t offset=(size_t)*count*4;
        details[offset]=(uint64_t)pids[i];
        details[offset+1]=info.ri_resident_size;
        details[offset+2]=info.ri_phys_footprint;
        details[offset+3]=strstr(path,"Decoder0.xpc/") ? 1 : strstr(path,"Decoder1.xpc/") ? 2 : 0;
        *rss+=info.ri_resident_size; *footprint+=info.ri_phys_footprint; *count+=1;
    }
    return missing;
}
