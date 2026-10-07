// Android GLES layer: opt-in workaround for missing framebuffer attachments.
// No libc/GL dependency: entry points come from Android's layer loader.
typedef void (*Attach)(unsigned, unsigned, unsigned, unsigned, int);
typedef unsigned (*Check)(unsigned);
static Attach attach;
static Check check;
static int same(const char *a, const char *b) {
    while (*a && *a == *b) { ++a; ++b; }
    return *a == *b;
}
static void validate_attachment(unsigned target, unsigned attachment,
                                unsigned texture_target, unsigned texture, int level) {
    Attach next = __atomic_load_n(&attach, __ATOMIC_RELAXED);
    Check validate = __atomic_load_n(&check, __ATOMIC_RELAXED);
    next(target, attachment, texture_target, texture, level);
    // Force driver attachment validation before the next draw. Keep all GL
    // arguments, shader code and render commands unchanged.
    if (validate) validate(target);
}
__attribute__((visibility("default")))
void AndroidGLESLayer_Initialize(void *id, void *(*lookup)(void *, const char *)) {
    __atomic_store_n(&attach, (Attach)lookup(id, "glFramebufferTexture2D"), __ATOMIC_RELAXED);
    __atomic_store_n(&check, (Check)lookup(id, "glCheckFramebufferStatus"), __ATOMIC_RELAXED);
}
__attribute__((visibility("default")))
void *AndroidGLESLayer_GetProcAddress(const char *name, void *next) {
    if (next && __atomic_load_n(&check, __ATOMIC_RELAXED) && same(name, "glFramebufferTexture2D")) {
        __atomic_store_n(&attach, (Attach)next, __ATOMIC_RELAXED);
        return (void *)validate_attachment;
    }
    return next;
}
