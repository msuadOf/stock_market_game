// 仅为 Debian 解包 sysroot 查找原版 WebKit GTK 辅助程序，不改变沙箱配置。
#define _GNU_SOURCE
#include <dlfcn.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef void *(*spawnv_t)(void *, const char *const *, void **);

void *g_subprocess_launcher_spawnv(void *launcher, const char *const *argv, void **error) {
    spawnv_t spawn = (spawnv_t)dlsym(RTLD_NEXT, "g_subprocess_launcher_spawnv");
    if (!spawn) abort();
    const char *prefix = "/usr/lib/x86_64-linux-gnu/webkit2gtk-4.1/";
    size_t offset = strlen(prefix);
    if (!argv || !argv[0] || strncmp(argv[0], prefix, offset) != 0)
        return spawn(launcher, argv, error);
    const char *file = argv[0] + offset;
    if (strcmp(file, "WebKitNetworkProcess") != 0 &&
        strcmp(file, "WebKitWebProcess") != 0 && strcmp(file, "WebKitGPUProcess") != 0)
        return spawn(launcher, argv, error);
    const char *root = getenv("LOCAL_WEBKIT_HELPER_DIR");
    if (!root || root[0] != '/') abort();
    char *path = NULL;
    if (asprintf(&path, "%s/%s", root, file) < 0 || access(path, X_OK) != 0) abort();
    size_t length = 0;
    while (argv[length]) ++length;
    const char **arguments = calloc(length + 1, sizeof(*arguments));
    if (!arguments) abort();
    memcpy(arguments, argv, length * sizeof(*arguments));
    arguments[0] = path;
    void *child = spawn(launcher, arguments, error);
    free(arguments);
    free(path);
    return child;
}
