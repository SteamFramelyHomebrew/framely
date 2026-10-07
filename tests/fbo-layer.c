// Verify actual compiled GLES entry points without needing a graphics context.
#include <assert.h>
#include <dlfcn.h>
#include <string.h>
static int calls, queries;
static void attachment(unsigned t, unsigned a, unsigned tt, unsigned id, int l) {
    assert(t==1 && a==2 && tt==3 && id==4 && l==5); ++calls;
}
static unsigned status(unsigned t) { assert(t==1 && calls==queries+1); ++queries; return 0x8cd5; }
static void *lookup(void *id, const char *name) {
    assert(id==(void*)42);
    if (!strcmp(name,"glFramebufferTexture2D")) return attachment;
    if (!strcmp(name,"glCheckFramebufferStatus")) return status;
    return 0;
}
static void *missing(void *id, const char *name) { (void)id; (void)name; return 0; }
int main(int argc, char **argv) {
    assert(argc==2);
    void *lib=dlopen(argv[1],RTLD_NOW); assert(lib);
    void (*init)(void*,void*(*)(void*,const char*))=dlsym(lib,"AndroidGLESLayer_Initialize");
    void *(*get)(const char*,void*)=dlsym(lib,"AndroidGLESLayer_GetProcAddress");
    assert(init && get);
    init((void*)42,lookup);
    assert(get("glDrawArrays",(void*)123)==(void*)123);
    assert(get("glFramebufferTexture2D",0)==0);
    void (*hook)(unsigned,unsigned,unsigned,unsigned,int)=get("glFramebufferTexture2D",attachment);
    hook(1,2,3,4,5); assert(calls==1 && queries==1);
    init(0,missing);
    assert(get("glFramebufferTexture2D",attachment)==attachment);
    dlclose(lib);
}
