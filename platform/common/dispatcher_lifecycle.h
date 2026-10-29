#ifndef PW_DISPATCHER_LIFECYCLE_H
#define PW_DISPATCHER_LIFECYCLE_H
/* Serial lifecycle only. Callbacks own platform resources; no mount/flush claim.
 * release MUST accept partially prepared state. Failed start is cleaned by the
 * adapter's eventual filesystem destroy; stop is called only after success. */
typedef struct PW_DISPATCHER_OPS {
    int (*prepare)(void *);
    int (*start)(void *);
    int (*wait)(void *);
    void (*stop)(void *);
    int (*release)(void *);
} PW_DISPATCHER_OPS;
enum { PW_LIFE_PREPARE=1, PW_LIFE_START=2, PW_LIFE_WAIT=4,
       PW_LIFE_RELEASE=8, PW_LIFE_INVALID=16 };
static unsigned PwDispatcherRun(const PW_DISPATCHER_OPS *ops, void *ctx)
{
    unsigned result=0;
    if (!ops || !ops->prepare || !ops->start || !ops->wait ||
        !ops->stop || !ops->release || !ctx) return PW_LIFE_INVALID;
    if (!ops->prepare(ctx)) result=PW_LIFE_PREPARE;
    else if (!ops->start(ctx)) result=PW_LIFE_START;
    else {
        if (!ops->wait(ctx)) result=PW_LIFE_WAIT;
        ops->stop(ctx);
    }
    if (!ops->release(ctx)) result |= PW_LIFE_RELEASE;
    return result;
}
#endif
