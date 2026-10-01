#define _DARWIN_C_SOURCE 1

/*
 * WorldDB M4-15 APFS sync probe.
 * Build and run only on macOS, against a directory on the APFS volume under
 * evaluation. This measures API latency and return values; it cannot prove
 * persistence across power loss.
 */
#if !defined(__APPLE__)
#error "This probe must be built and run on macOS."
#endif

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mount.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

#ifndef F_FULLFSYNC
#error "The active macOS SDK does not define F_FULLFSYNC."
#endif

#define PAYLOAD_SIZE 4096

static int64_t monotonic_ns(void) {
  struct timespec ts;
  if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) {
    perror("clock_gettime");
    exit(EXIT_FAILURE);
  }
  return (int64_t)ts.tv_sec * INT64_C(1000000000) + ts.tv_nsec;
}

static void emit_result(const char *operation, long iteration, int result,
                        int error_number, int64_t elapsed_ns) {
  printf("%s,%ld,%d,%d,%" PRId64 "\n", operation, iteration, result,
         error_number, elapsed_ns);
}

static int write_payload(int fd, const unsigned char *payload, size_t size) {
  size_t written = 0;
  while (written < size) {
    ssize_t count = write(fd, payload + written, size - written);
    if (count < 0 && errno == EINTR) {
      continue;
    }
    if (count <= 0) {
      return -1;
    }
    written += (size_t)count;
  }
  return 0;
}

static int measure_one(const char *directory, const char *operation,
                       long iteration, int full_sync) {
  char path[PATH_MAX];
  int path_length = snprintf(path, sizeof(path),
                             "%s/worlddb-m4-15-XXXXXX", directory);
  if (path_length < 0 || (size_t)path_length >= sizeof(path)) {
    fprintf(stderr, "temporary path is too long\n");
    return -1;
  }

  int fd = mkstemp(path);
  if (fd < 0) {
    perror("mkstemp");
    return -1;
  }

  unsigned char payload[PAYLOAD_SIZE];
  for (size_t index = 0; index < sizeof(payload); ++index) {
    payload[index] = (unsigned char)((iteration * 31L + (long)index * 17L +
                                      (full_sync ? 1L : 0L)) &
                                     0xffL);
  }
  if (write_payload(fd, payload, sizeof(payload)) != 0) {
    perror("write");
    close(fd);
    unlink(path);
    return -1;
  }

  int64_t start = monotonic_ns();
  errno = 0;
  int result = full_sync ? fcntl(fd, F_FULLFSYNC) : fsync(fd);
  int error_number = errno;
  int64_t elapsed = monotonic_ns() - start;
  emit_result(operation, iteration, result, error_number, elapsed);

  int close_result = close(fd);
  int unlink_result = unlink(path);
  if (close_result != 0 || unlink_result != 0) {
    perror("close/unlink temporary sample");
    return -1;
  }
  return 0;
}

static int measure_bad_descriptor(const char *directory) {
  char path[PATH_MAX];
  int path_length = snprintf(path, sizeof(path),
                             "%s/worlddb-m4-15-fault-XXXXXX", directory);
  if (path_length < 0 || (size_t)path_length >= sizeof(path)) {
    fprintf(stderr, "temporary path is too long\n");
    return -1;
  }

  int fd = mkstemp(path);
  if (fd < 0) {
    perror("mkstemp for injected EBADF");
    return -1;
  }
  if (close(fd) != 0 || unlink(path) != 0) {
    perror("close/unlink injected-fault file");
    return -1;
  }

  errno = 0;
  int64_t start = monotonic_ns();
  int result = fsync(fd);
  int error_number = errno;
  emit_result("fault_fsync_ebadf", 0, result, error_number,
              monotonic_ns() - start);

  errno = 0;
  start = monotonic_ns();
  result = fcntl(fd, F_FULLFSYNC);
  error_number = errno;
  emit_result("fault_fullfsync_ebadf", 0, result, error_number,
              monotonic_ns() - start);
  return 0;
}

int main(int argc, char **argv) {
  if (argc != 3) {
    fprintf(stderr, "usage: %s APFS_DIRECTORY ITERATIONS\n", argv[0]);
    return EXIT_FAILURE;
  }

  char *end = NULL;
  errno = 0;
  long iterations = strtol(argv[2], &end, 10);
  if (errno != 0 || end == argv[2] || *end != '\0' || iterations < 1 ||
      iterations > 10000) {
    fprintf(stderr, "ITERATIONS must be an integer from 1 to 10000\n");
    return EXIT_FAILURE;
  }

  struct stat directory_info;
  if (stat(argv[1], &directory_info) != 0) {
    perror("stat directory");
    return EXIT_FAILURE;
  }
  if (!S_ISDIR(directory_info.st_mode)) {
    fprintf(stderr, "APFS_DIRECTORY must name a directory\n");
    return EXIT_FAILURE;
  }

  struct statfs volume;
  if (statfs(argv[1], &volume) != 0) {
    perror("statfs");
    return EXIT_FAILURE;
  }
  if (strcmp(volume.f_fstypename, "apfs") != 0) {
    fprintf(stderr, "refusing non-APFS path (reported filesystem: %s)\n",
            volume.f_fstypename);
    return EXIT_FAILURE;
  }

  fprintf(stderr, "filesystem=%s\nmountpoint=%s\niterations=%ld\n",
          volume.f_fstypename, volume.f_mntonname, iterations);
  puts("operation,iteration,return,errno,elapsed_ns");

  for (long iteration = 0; iteration < iterations; ++iteration) {
    if (measure_one(argv[1], "fsync", iteration, 0) != 0 ||
        measure_one(argv[1], "fullfsync", iteration, 1) != 0) {
      return EXIT_FAILURE;
    }
  }
  if (measure_bad_descriptor(argv[1]) != 0) {
    return EXIT_FAILURE;
  }
  if (fflush(stdout) != 0) {
    perror("flush CSV");
    return EXIT_FAILURE;
  }
  return EXIT_SUCCESS;
}
