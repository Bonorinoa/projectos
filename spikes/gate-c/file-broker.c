#define _DARWIN_C_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

#define READ_LIMIT (16 * 1024)

/* Every lookup is relative to an already-open directory. Rename/symlink swaps
 * cannot retarget that descriptor. No canonicalize/check-then-open operation. */
static int scoped_open(int root, const char *path) {
  if (!path || !*path || *path == '/') { errno = EINVAL; return -1; }
  char *copy = strdup(path);
  if (!copy) return -1;
  int current = dup(root);
  char *save = NULL;
  char *component = strtok_r(copy, "/", &save);
  while (component && current >= 0) {
    if (!strcmp(component, "..") || !strcmp(component, ".")) {
      close(current); current = -1; errno = EINVAL; break;
    }
    char *next = strtok_r(NULL, "/", &save);
    int flags = O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK;
    if (next) flags |= O_DIRECTORY;
    int opened = openat(current, component, flags);
    int saved_errno = errno;
    close(current);
    current = opened;
    errno = saved_errno;
    component = next;
  }
  free(copy);
  return current;
}

#ifndef BROKER_LIBRARY
static int denied(const char *code) {
  printf("{\"ok\":false,\"error\":{\"code\":\"%s\",\"message\":\"native scoped read denied\"}}\n", code);
  return 0;
}

int main(int argc, char **argv) {
  if (argc != 5) return 64;
  const char *path = argv[4];
  if (*path == '/') return denied("CAP_PATH_ABSOLUTE");
  /* Reject traversal before opening anything, including a/../b. */
  char *validation = strdup(path);
  if (!validation) return 70;
  char *save = NULL;
  for (char *p = strtok_r(validation, "/", &save); p; p = strtok_r(NULL, "/", &save)) {
    if (!strcmp(p, "..") || !strcmp(p, ".")) { free(validation); return denied("CAP_PATH_ESCAPE"); }
  }
  free(validation);
  int root = !strcmp(argv[1], "--fd") ? dup(3) : open(argv[1], O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
  if (root < 0) return denied("CAP_ROOT_CHANGED");
  struct stat anchor;
  if (fstat(root, &anchor) || (uintmax_t)anchor.st_dev != strtoull(argv[2], NULL, 10) ||
      (uintmax_t)anchor.st_ino != strtoull(argv[3], NULL, 10)) {
    close(root); return denied("CAP_ROOT_CHANGED");
  }
  int file = scoped_open(root, path);
  close(root);
  if (file < 0) return denied(errno == ELOOP || errno == ENOTDIR ? "CAP_SYMLINK" : "CAP_OPEN_DENIED");
  struct stat before, after;
  if (fstat(file, &before) || !S_ISREG(before.st_mode)) { close(file); return denied("CAP_NOT_FILE"); }
  unsigned char buffer[READ_LIMIT + 1];
  size_t used = 0;
  while (used < sizeof(buffer)) {
    ssize_t n = read(file, buffer + used, sizeof(buffer) - used);
    if (n < 0 && errno == EINTR) continue;
    if (n < 0) { close(file); return denied("CAP_READ_DENIED"); }
    if (!n) break;
    used += (size_t)n;
  }
  if (used > READ_LIMIT) { close(file); return denied("CAP_SIZE_LIMIT"); }
  if (fstat(file, &after) || before.st_dev != after.st_dev || before.st_ino != after.st_ino) {
    close(file); return denied("CAP_IDENTITY_CHANGED");
  }
  close(file);
  printf("{\"ok\":true,\"result\":{\"hex\":\"");
  for (size_t i = 0; i < used; i++) printf("%02x", buffer[i]);
  printf("\",\"bytes\":%zu,\"device\":\"%ju\",\"inode\":\"%ju\"}}\n", used,
         (uintmax_t)before.st_dev, (uintmax_t)before.st_ino);
  return 0;
}
#endif
