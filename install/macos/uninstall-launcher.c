/*
 * The compiled main executable of "Uninstall OpenBark.app" in signed builds.
 *
 * Apple's notary service and stapler expect an app's main executable to be
 * Mach-O, not a script (ADR-0020, decision 3). scripts/macos-dmg.sh compiles
 * this file in signing mode, moves the uninstall script to
 * Contents/Resources/uninstall.sh, and puts this launcher at
 * Contents/MacOS/uninstall. It runs the script with /bin/bash and passes any
 * arguments through. Nothing else.
 */
#include <libgen.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

int main(int argc, char *argv[]) {
    char raw[PATH_MAX];
    uint32_t size = sizeof raw;
    if (_NSGetExecutablePath(raw, &size) != 0) {
        fputs("Uninstall OpenBark: executable path too long\n", stderr);
        return 1;
    }
    char exe[PATH_MAX];
    if (realpath(raw, exe) == NULL) {
        perror("Uninstall OpenBark: realpath");
        return 1;
    }
    /* .../Uninstall OpenBark.app/Contents/MacOS/uninstall -> .../Contents */
    char macos_dir[PATH_MAX];
    strlcpy(macos_dir, dirname(exe), sizeof macos_dir);
    char contents[PATH_MAX];
    strlcpy(contents, dirname(macos_dir), sizeof contents);

    char script[PATH_MAX];
    if (snprintf(script, sizeof script, "%s/Resources/uninstall.sh", contents) >= (int)sizeof script) {
        fputs("Uninstall OpenBark: script path too long\n", stderr);
        return 1;
    }

    char **args = calloc((size_t)argc + 2, sizeof *args);
    if (args == NULL) {
        return 1;
    }
    args[0] = "/bin/bash";
    args[1] = script;
    for (int i = 1; i < argc; i++) {
        args[i + 1] = argv[i];
    }
    args[argc + 1] = NULL;
    execv("/bin/bash", args);
    perror("Uninstall OpenBark: could not start /bin/bash");
    return 1;
}
