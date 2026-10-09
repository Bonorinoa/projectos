#define _DARWIN_C_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <stdio.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>

/* This is a native App Sandbox probe, not the TypeScript worker. It attempts
 * operations normally available to a malicious worker to measure the boundary. */
int main(int argc, char **argv) {
  if (argc != 2) return 64;
  int file = open(argv[1], O_RDONLY);
  int file_error = file < 0 ? errno : 0;
  if (file >= 0) close(file);
  int socket_fd = socket(AF_INET, SOCK_STREAM, 0), network_error = 0;
  if (socket_fd < 0) network_error = errno;
  else {
    struct sockaddr_in address = { .sin_family = AF_INET, .sin_port = htons(9) };
    inet_pton(AF_INET, "127.0.0.1", &address.sin_addr);
    if (connect(socket_fd, (struct sockaddr *)&address, sizeof(address))) network_error = errno;
    close(socket_fd);
  }
  struct addrinfo *addresses = NULL;
  int dns_error = getaddrinfo("projectos-probe.invalid", NULL, NULL, &addresses);
  if (addresses) freeaddrinfo(addresses);
  int status = 0;
  pid_t child = fork();
  int fork_error = child < 0 ? errno : 0;
  if (child == 0) { execl("/bin/sh", "sh", "-c", "exit 42", (char *)NULL); _exit(127); }
  if (child > 0) waitpid(child, &status, 0);
  printf("{\"ambientFileErrno\":%d,\"localhostErrno\":%d,\"dnsReturn\":%d,\"forkErrno\":%d,\"childExit\":%d}\n",
    file_error, network_error, dns_error, fork_error, child > 0 && WIFEXITED(status) ? WEXITSTATUS(status) : -1);
  return 0;
}
