#define BROKER_LIBRARY
#include "file-broker.c"
#include <signal.h>
#include <sys/wait.h>
#include <time.h>

int main(int argc, char **argv) {
  if (argc != 3) return 64;
  int root = open(argv[1], O_RDONLY | O_DIRECTORY | O_NOFOLLOW);
  if (root < 0) return 70;
  /* In a disposable fixture, move the real directory aside and replace its
   * original name with an outside symlink while the reader continuously runs. */
  pid_t attacker = fork();
  if (attacker < 0) return 70;
  if (attacker == 0) {
    for (;;) {
      renameat(root, "slot", root, "parked");
      symlinkat(argv[2], root, "slot");
      unlinkat(root, "slot", 0);
      renameat(root, "parked", root, "slot");
    }
  }
  unsigned allowed = 0, denied = 0, escaped = 0;
  for (unsigned i = 0; i < 50000; i++) {
    int file = scoped_open(root, "slot/data.txt");
    if (file < 0) { denied++; continue; }
    char value[64] = {0};
    ssize_t n = read(file, value, sizeof(value) - 1);
    close(file);
    if (n == 7 && !memcmp(value, "INSIDE\n", 7)) allowed++;
    else escaped++;
  }
  kill(attacker, SIGKILL);
  waitpid(attacker, NULL, 0);
  unlinkat(root, "slot", 0);
  renameat(root, "parked", root, "slot");
  close(root);
  printf("{\"attempts\":50000,\"allowed\":%u,\"denied\":%u,\"escaped\":%u}\n", allowed, denied, escaped);
  return escaped || !allowed || !denied ? 1 : 0;
}
