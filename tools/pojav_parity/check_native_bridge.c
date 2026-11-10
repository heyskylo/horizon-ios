/* Runtime linkage check for the shipping simulator framework's public C ABI.
 * No UIApplication, player state, accounts or game files are opened. */
#include <dlfcn.h>
#include <stdint.h>
#include <stdio.h>

typedef uint32_t (*GesturePoll)(uint64_t, uint32_t, double, double, double, double *, uint32_t);
typedef void (*SetContext)(uint64_t);

/* An empty queue returns no records for a full, a too-small and a null buffer, and writes nothing. */
static int check(void *framework, const char *name, uint32_t record) {
    GesturePoll poll = (GesturePoll)dlsym(framework, name);
    if (!poll) { fprintf(stderr, "%s: export missing\n", name); return 1; }
    double values[12];
    for (int i = 0; i < 12; i++) values[i] = 123;
    if (poll(UINT64_C(0xabc123456789), 2, 600, 300, 4, values, record) != 0 ||
        poll(UINT64_C(0xabc123456789), 4, 600, 300, 4, values, record - 1) != 0 ||
        poll(UINT64_C(0xabc123456789), 2, 600, 300, 4, NULL, record) != 0) {
        fprintf(stderr, "%s: unexpected bridge count\n", name); return 1;
    }
    for (int i = 0; i < 12; i++) if (values[i] != 123) {
        fprintf(stderr, "%s: empty queue wrote past returned records\n", name); return 1;
    }
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2) { fprintf(stderr, "usage: check_native_bridge FRAMEWORK_BINARY\n"); return 2; }
    void *framework = dlopen(argv[1], RTLD_NOW);
    if (!framework) { fprintf(stderr, "framework load failed: %s\n", dlerror()); return 1; }
    /* The game reads eleven doubles per record through v2 and names the click context a
     * gesture belongs to; v1 (six doubles) stays exported for older bundled readers. */
    if (check(framework, "horizon_touch_gestures_v2", 11) || check(framework, "horizon_touch_gestures_v1", 6)) return 1;
    SetContext set = (SetContext)dlsym(framework, "horizon_touch_set_context_v1");
    if (!set) { fprintf(stderr, "horizon_touch_set_context_v1: export missing\n"); return 1; }
    set(1);
    if (check(framework, "horizon_touch_gestures_v2", 11)) return 1;
    puts("Shipping framework loaded; gesture C ABI and bounds checks passed.");
    return 0;
}
