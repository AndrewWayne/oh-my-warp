// SPDX-License-Identifier: AGPL-3.0-only
#include <libproc.h>
#include <string.h>
#include <sys/proc_info.h>
#include <sys/socket.h>

// Use SDK layouts instead of duplicating socket_info's platform unions in Rust.
int omw_codex_peer_socket(int pid, int fd, char *path, size_t capacity) {
    struct socket_fdinfo info = {0};
    if (proc_pidfdinfo(pid, fd, PROC_PIDFDSOCKETINFO, &info, sizeof(info)) != sizeof(info)) {
        return -1;
    }
    if (info.psi.soi_family != AF_UNIX || info.psi.soi_kind != SOCKINFO_UN ||
        info.psi.soi_type != SOCK_STREAM || !(info.psi.soi_state & SOI_S_ISCONNECTED)) {
        return 0;
    }
    const struct sockaddr_un *address = &info.psi.soi_proto.pri_un.unsi_caddr.ua_sun;
    size_t length = strnlen(address->sun_path, sizeof(address->sun_path));
    if (!length || address->sun_path[0] != '/' || length == sizeof(address->sun_path) || length >= capacity) {
        return 0;
    }
    memcpy(path, address->sun_path, length);
    return (int)length;
}
