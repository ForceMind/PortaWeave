#ifndef PW_DISPATCHER_LIFECYCLE_CONTRACT_H
#define PW_DISPATCHER_LIFECYCLE_CONTRACT_H
#include "dispatcher_lifecycle.h"
#include <string.h>
#include <stdio.h>
typedef struct { unsigned fail; char trace[8]; unsigned at; } PW_LIFE_MOCK;
static void PwLifeTrace(PW_LIFE_MOCK *m, char c) { m->trace[m->at++]=c; }
static int PwLifePrepare(void *ctx) { PW_LIFE_MOCK *m=ctx; PwLifeTrace(m,'P'); return !(m->fail & PW_LIFE_PREPARE); }
static int PwLifeStart(void *ctx) { PW_LIFE_MOCK *m=ctx; PwLifeTrace(m,'S'); return !(m->fail & PW_LIFE_START); }
static int PwLifeWait(void *ctx) { PW_LIFE_MOCK *m=ctx; PwLifeTrace(m,'W'); return !(m->fail & PW_LIFE_WAIT); }
static void PwLifeStop(void *ctx) { PwLifeTrace(ctx,'T'); }
static int PwLifeRelease(void *ctx) { PW_LIFE_MOCK *m=ctx; PwLifeTrace(m,'R'); return !(m->fail & PW_LIFE_RELEASE); }
static int PwLifecycleContract(void)
{
    const PW_DISPATCHER_OPS ops={PwLifePrepare,PwLifeStart,PwLifeWait,PwLifeStop,PwLifeRelease};
    const unsigned masks[]={0,1,2,4,8,1|8,2|8,4|8};
    const char *traces[]={"PSWTR","PR","PSR","PSWTR","PSWTR","PR","PSR","PSWTR"};
    unsigned i;
    PW_LIFE_MOCK m;
    for(i=0;i<sizeof(masks)/sizeof(masks[0]);++i) {
        memset(&m,0,sizeof(m)); m.fail=masks[i];
        if (PwDispatcherRun(&ops,&m)!=masks[i] || strcmp(m.trace,traces[i])) {
            fprintf(stderr,"dispatcher lifecycle case %u failed\n",i); return 1;
        }
    }
    if (PwDispatcherRun(NULL,&m)!=PW_LIFE_INVALID ||
        PwDispatcherRun(&ops,NULL)!=PW_LIFE_INVALID) return 1;
    puts("DISPATCHER_LIFECYCLE_CONTRACT_OK (8 ordered fault cases; no driver/mount)");
    return 0;
}
#endif
