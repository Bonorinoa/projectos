#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/event.h>
#include <sys/resource.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

static volatile sig_atomic_t worker_pid = -1;

static void stop_tree(int signal_number) {
  (void)signal_number;
  if (worker_pid > 0) kill(-worker_pid, SIGKILL);
}

static int write_pid_file(const char *path, pid_t pid) {
  FILE *file = fopen(path, "w");
  if (!file) return -1;
  fprintf(file, "%d\n", pid);
  if (fclose(file) != 0) return -1;
  return 0;
}

int main(int argc, char **argv) {
  if (argc < 8) {
    fprintf(stderr, "usage: guardian PARENT_PID PID_FILE CPU_SECONDS ADDRESS_SPACE_BYTES FILE_BYTES command [args...]\n");
    return 64;
  }

  const pid_t parent_pid = (pid_t)strtol(argv[1], NULL, 10);
  const char *pid_file = argv[2];
  const rlim_t cpu_seconds = (rlim_t)strtoull(argv[3], NULL, 10);
  const rlim_t address_space = (rlim_t)strtoull(argv[4], NULL, 10);
  const rlim_t file_bytes = (rlim_t)strtoull(argv[5], NULL, 10);

  signal(SIGTERM, stop_tree);
  signal(SIGINT, stop_tree);
  signal(SIGHUP, stop_tree);

  pid_t child = fork();
  if (child < 0) {
    perror("fork");
    return 70;
  }
  if (child == 0) {
    if (setpgid(0, 0) != 0) {
      perror("setpgid");
      _exit(70);
    }
    struct rlimit cpu = { cpu_seconds, cpu_seconds };
    struct rlimit as = { address_space, address_space };
    struct rlimit fsize = { file_bytes, file_bytes };
    if (setrlimit(RLIMIT_CPU, &cpu) != 0) perror("setrlimit RLIMIT_CPU");
    if (setrlimit(RLIMIT_AS, &as) != 0) perror("setrlimit RLIMIT_AS");
    if (setrlimit(RLIMIT_FSIZE, &fsize) != 0) perror("setrlimit RLIMIT_FSIZE");
    execvp(argv[6], &argv[6]);
    perror("execvp");
    _exit(127);
  }

  worker_pid = child;
  if (write_pid_file(pid_file, child) != 0) {
    perror("pid file");
    stop_tree(0);
    return 74;
  }

  int queue = kqueue();
  if (queue < 0) {
    perror("kqueue");
    stop_tree(0);
    return 70;
  }

  struct kevent changes[2];
  EV_SET(&changes[0], parent_pid, EVFILT_PROC, EV_ADD | EV_ENABLE, NOTE_EXIT, 0, NULL);
  EV_SET(&changes[1], child, EVFILT_PROC, EV_ADD | EV_ENABLE, NOTE_EXIT, 0, NULL);
  if (kevent(queue, changes, 2, NULL, 0, NULL) < 0) {
    perror("kevent register");
    stop_tree(0);
    return 70;
  }

  while (1) {
    struct kevent event;
    int count = kevent(queue, NULL, 0, &event, 1, NULL);
    if (count < 0) {
      if (errno == EINTR) continue;
      perror("kevent wait");
      stop_tree(0);
      return 70;
    }
    if ((pid_t)event.ident == parent_pid) {
      stop_tree(0);
      waitpid(child, NULL, 0);
      return 0;
    }
    if ((pid_t)event.ident == child) {
      int status = 0;
      waitpid(child, &status, 0);
      if (WIFEXITED(status)) return WEXITSTATUS(status);
      if (WIFSIGNALED(status)) return 128 + WTERMSIG(status);
      return 70;
    }
  }
}
