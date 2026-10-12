/*
 * The POSIX socket names, as real symbols, for the Rust standard library.
 *
 * ESP-IDF's lwIP uses the `lwip_`-prefixed function names and offers the POSIX spellings through its
 * own headers, which define them as `static inline` functions (`lwip/port/include` and the wrapper in
 * `components/lwip/include`). C sees those; Rust cannot — a compiled program has no C compiler, and a
 * `static inline` function leaves no symbol behind. So the NAS SSH client's `std::net::TcpStream`,
 * which asks the linker for `socket`, would find nothing: every archive in this image defines
 * `lwip_socket`, and none defines `socket`.
 *
 * This file is those names as actual functions, each forwarding to the lwIP implementation the rest of
 * the image already links. It is deliberately the socket lifecycle and name resolution only. The
 * byte-wise I/O a session needs (`read`, `write`, `close`, `poll`, `fcntl`, `select`) already exists
 * under those names, because ESP-IDF registers lwIP sockets with its VFS, so the ordinary libc call
 * dispatches to the socket for a socket descriptor and this file has no business redefining it.
 *
 * Nothing here is SSH. The session itself is Rust — `pomelo-firmware/firmware/pomelo-hal-esp32/src/nas.rs`
 * over the `sunset` crate — and this is the handful of libc spellings it needs to reach the network at
 * all. It lives in `board_hal` because that component is already the FFI boundary, and because its
 * `WHOLE_ARCHIVE` is what puts these definitions into the image before the Rust archive is read.
 *
 * The lwIP prototypes are declared here rather than by including `lwip/sockets.h`, and that is the
 * whole reason this file cannot include it: the header defines these very names as `static inline`,
 * and a definition and a declaration of the same function are a redefinition. Only pointers to
 * `struct sockaddr` and `struct addrinfo` cross this boundary — passed straight back to lwIP, never
 * looked into — so neither struct needs a definition here.
 */
#include <stddef.h>

typedef int ssize_t;
typedef unsigned int socklen_t;

struct sockaddr;
struct addrinfo;

extern int lwip_socket(int domain, int type, int protocol);
extern int lwip_connect(int s, const struct sockaddr *name, socklen_t namelen);
extern int lwip_bind(int s, const struct sockaddr *name, socklen_t namelen);
extern int lwip_listen(int s, int backlog);
extern int lwip_accept(int s, struct sockaddr *addr, socklen_t *addrlen);
extern int lwip_shutdown(int s, int how);
extern int lwip_setsockopt(int s, int level, int optname, const void *optval, socklen_t optlen);
extern int lwip_getsockopt(int s, int level, int optname, void *optval, socklen_t *optlen);
extern int lwip_getsockname(int s, struct sockaddr *name, socklen_t *namelen);
extern int lwip_getpeername(int s, struct sockaddr *name, socklen_t *namelen);
extern ssize_t lwip_send(int s, const void *dataptr, size_t size, int flags);
extern ssize_t lwip_recv(int s, void *mem, size_t len, int flags);
extern ssize_t lwip_sendto(int s, const void *dataptr, size_t size, int flags, const struct sockaddr *to, socklen_t tolen);
extern ssize_t lwip_recvfrom(int s, void *mem, size_t len, int flags, struct sockaddr *from, socklen_t *fromlen);
extern int lwip_getaddrinfo(const char *nodename, const char *servname, const struct addrinfo *hints, struct addrinfo **res);
extern void lwip_freeaddrinfo(struct addrinfo *ai);

int socket(int domain, int type, int protocol);
int connect(int s, const struct sockaddr *name, socklen_t namelen);
int bind(int s, const struct sockaddr *name, socklen_t namelen);
int listen(int s, int backlog);
int accept(int s, struct sockaddr *addr, socklen_t *addrlen);
int shutdown(int s, int how);
int setsockopt(int s, int level, int optname, const void *optval, socklen_t optlen);
int getsockopt(int s, int level, int optname, void *optval, socklen_t *optlen);
int getsockname(int s, struct sockaddr *name, socklen_t *namelen);
int getpeername(int s, struct sockaddr *name, socklen_t *namelen);
ssize_t send(int s, const void *dataptr, size_t size, int flags);
ssize_t recv(int s, void *mem, size_t len, int flags);
ssize_t sendto(int s, const void *dataptr, size_t size, int flags, const struct sockaddr *to, socklen_t tolen);
ssize_t recvfrom(int s, void *mem, size_t len, int flags, struct sockaddr *from, socklen_t *fromlen);
int getaddrinfo(const char *nodename, const char *servname, const struct addrinfo *hints, struct addrinfo **res);
void freeaddrinfo(struct addrinfo *ai);

int socket(int domain, int type, int protocol) { return lwip_socket(domain, type, protocol); }

int connect(int s, const struct sockaddr *name, socklen_t namelen) { return lwip_connect(s, name, namelen); }

int bind(int s, const struct sockaddr *name, socklen_t namelen) { return lwip_bind(s, name, namelen); }

int listen(int s, int backlog) { return lwip_listen(s, backlog); }

int accept(int s, struct sockaddr *addr, socklen_t *addrlen) { return lwip_accept(s, addr, addrlen); }

int shutdown(int s, int how) { return lwip_shutdown(s, how); }

int setsockopt(int s, int level, int optname, const void *optval, socklen_t optlen)
{
    return lwip_setsockopt(s, level, optname, optval, optlen);
}

int getsockopt(int s, int level, int optname, void *optval, socklen_t *optlen)
{
    return lwip_getsockopt(s, level, optname, optval, optlen);
}

int getsockname(int s, struct sockaddr *name, socklen_t *namelen)
{
    return lwip_getsockname(s, name, namelen);
}

int getpeername(int s, struct sockaddr *name, socklen_t *namelen)
{
    return lwip_getpeername(s, name, namelen);
}

ssize_t send(int s, const void *dataptr, size_t size, int flags)
{
    return lwip_send(s, dataptr, size, flags);
}

ssize_t recv(int s, void *mem, size_t len, int flags) { return lwip_recv(s, mem, len, flags); }

ssize_t sendto(int s, const void *dataptr, size_t size, int flags, const struct sockaddr *to, socklen_t tolen)
{
    return lwip_sendto(s, dataptr, size, flags, to, tolen);
}

ssize_t recvfrom(int s, void *mem, size_t len, int flags, struct sockaddr *from, socklen_t *fromlen)
{
    return lwip_recvfrom(s, mem, len, flags, from, fromlen);
}

int getaddrinfo(const char *nodename, const char *servname, const struct addrinfo *hints, struct addrinfo **res)
{
    return lwip_getaddrinfo(nodename, servname, hints, res);
}

void freeaddrinfo(struct addrinfo *ai) { lwip_freeaddrinfo(ai); }
