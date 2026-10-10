/* Offline RGB24 generation using the shared encoder translated by wasm2c.
 * Build together with generator.c and WABT's runtime, using C11 for thread-local
 * runtime state, -pthread, -fno-fast-math, -ffp-contract=off and -frounding-math.
 */
#define _POSIX_C_SOURCE 200809L
#define _DARWIN_C_SOURCE 1
#define _DEFAULT_SOURCE 1
#include "generator.h"
#include <errno.h>
#include <fcntl.h>
#include <fenv.h>
#include <pthread.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

enum { BLOCK_COUNT = 4096, BLOCK_BYTES = 16464, MAX_THREADS = 256 };
static volatile sig_atomic_t interrupted;

typedef struct {
  const char *output;
  unsigned start, end, threads;
} Options;

typedef struct {
  Options options;
  pthread_mutex_t mutex;
  unsigned next, completed, restored;
  char error[512];
  struct timespec started;
} Queue;

static void handle_signal(int number) { interrupted = number; }

static uint32_t read_u32(const uint8_t *bytes) {
  return (uint32_t)bytes[0] | ((uint32_t)bytes[1] << 8) |
         ((uint32_t)bytes[2] << 16) | ((uint32_t)bytes[3] << 24);
}

static double elapsed(const Queue *queue) {
  struct timespec now;
  clock_gettime(CLOCK_MONOTONIC, &now);
  return (double)(now.tv_sec - queue->started.tv_sec) +
         (double)(now.tv_nsec - queue->started.tv_nsec) / 1e9;
}

static uint8_t *memory_range(w2c_generator *instance, uint32_t pointer,
                             uint32_t length) {
  wasm_rt_memory_t *memory = w2c_generator_memory(instance);
  if ((uint64_t)pointer + length > memory->size) return NULL;
  return memory->data + pointer;
}

static const uint8_t *result(w2c_generator *instance, uint32_t *length) {
  uint32_t pointer = w2c_generator_mup_result_ptr(instance);
  *length = w2c_generator_mup_result_len(instance);
  return memory_range(instance, pointer, *length);
}

static void wasm_error(w2c_generator *instance, char *error, size_t capacity) {
  uint32_t length = w2c_generator_mup_error_len(instance);
  uint32_t pointer = w2c_generator_mup_error_ptr(instance);
  const uint8_t *bytes = memory_range(instance, pointer, length);
  if (!bytes) {
    snprintf(error, capacity, "Invalid encoder error buffer");
    return;
  }
  if (length >= capacity) length = (uint32_t)capacity - 1;
  memcpy(error, bytes, length);
  error[length] = '\0';
}

static char *path_for(const char *directory, const char *filename) {
  size_t size = strlen(directory) + strlen(filename) + 2;
  char *path = malloc(size);
  if (path) snprintf(path, size, "%s/%s", directory, filename);
  return path;
}

static int write_all(int file, const uint8_t *bytes, size_t length) {
  while (length) {
    ssize_t written = write(file, bytes, length);
    if (written < 0 && errno == EINTR) continue;
    if (written <= 0) return -1;
    bytes += written;
    length -= (size_t)written;
  }
  return 0;
}

/* Only the holder of generation.lock may recover these reserved temporaries. */
static int publish(const char *directory, const char *filename,
                   const uint8_t *bytes, size_t length, char *error,
                   size_t capacity) {
  char *final = path_for(directory, filename);
  size_t partial_size = strlen(filename) + 9;
  char *partial_name = malloc(partial_size);
  char *partial = NULL;
  int file = -1, success = 0, owned = 0, saved_errno = 0;
  if (!final || !partial_name) {
    saved_errno = ENOMEM;
    goto done;
  }
  snprintf(partial_name, partial_size, "%s.partial", filename);
  partial = path_for(directory, partial_name);
  if (!partial) {
    saved_errno = ENOMEM;
    goto done;
  }
  if (unlink(partial) < 0 && errno != ENOENT) goto failed;
  file = open(partial, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW, 0600);
  if (file < 0) goto failed;
  owned = 1;
  if (write_all(file, bytes, length) < 0 || fsync(file) < 0) goto failed;
  if (close(file) < 0) {
    file = -1;
    goto failed;
  }
  file = -1;
  if (rename(partial, final) < 0) goto failed;
  owned = 0;
  success = 1;
  {
    int dir = open(directory, O_RDONLY | O_DIRECTORY);
    if (dir >= 0) {
      (void)fsync(dir);
      close(dir);
    }
  }
  goto done;
failed:
  saved_errno = errno;
done:
  if (file >= 0) close(file);
  if (owned) unlink(partial);
  if (!success)
    snprintf(error, capacity, "Cannot publish %s: %s", filename,
             strerror(saved_errno ? saved_errno : EIO));
  free(partial);
  free(partial_name);
  free(final);
  return success ? 0 : -1;
}

/* Return 1 for a validated block, 0 for a cache miss, -1 for an I/O failure. */
static int restore(w2c_generator *instance, const char *directory,
                   unsigned number, char *error, size_t capacity) {
  char filename[32];
  struct stat status;
  uint32_t allocation = 0, length;
  uint8_t *bytes;
  size_t received = 0;
  int file, valid = 0;
  snprintf(filename, sizeof(filename), "block-%04u.lut", number);
  char *path = path_for(directory, filename);
  if (!path) {
    snprintf(error, capacity, "Out of memory reading %s", filename);
    return -1;
  }
  file = open(path, O_RDONLY | O_NOFOLLOW);
  free(path);
  if (file < 0) {
    if (errno == ENOENT) return 0;
    snprintf(error, capacity, "Cannot read %s: %s", filename, strerror(errno));
    return -1;
  }
  if (fstat(file, &status) < 0 || !S_ISREG(status.st_mode)) {
    snprintf(error, capacity, "Expected regular file %s", filename);
    valid = -1;
    goto done;
  }
  if (status.st_size != BLOCK_BYTES) goto done;
  allocation = w2c_generator_mup_alloc(instance, BLOCK_BYTES);
  bytes = memory_range(instance, allocation, BLOCK_BYTES);
  if (!allocation || !bytes) {
    snprintf(error, capacity, "Cannot allocate block validation buffer");
    valid = -1;
    goto done;
  }
  while (received < BLOCK_BYTES) {
    ssize_t count = read(file, bytes + received, BLOCK_BYTES - received);
    if (count < 0 && errno == EINTR) continue;
    if (count < 0) {
      snprintf(error, capacity, "Cannot read %s: %s", filename, strerror(errno));
      valid = -1;
      goto done;
    }
    if (count == 0) goto done;
    received += (size_t)count;
  }
  {
    uint8_t extra;
    ssize_t count;
    do { count = read(file, &extra, 1); } while (count < 0 && errno == EINTR);
    if (count != 0) goto done;
  }
  if (w2c_generator_mup_lut_validate(instance, allocation, BLOCK_BYTES) != 0)
    goto done;
  {
    const uint8_t *value = result(instance, &length);
    valid = value && length == 4 && read_u32(value) == number;
  }
done:
  if (allocation) w2c_generator_mup_free(instance, allocation, BLOCK_BYTES);
  close(file);
  return valid;
}

static int identity_result(w2c_generator *instance, uint32_t status,
                           char destination[65], char *error, size_t capacity) {
  uint32_t length;
  const uint8_t *bytes;
  if (status) {
    wasm_error(instance, error, capacity);
    return -1;
  }
  bytes = result(instance, &length);
  if (!bytes || length != 64) {
    snprintf(error, capacity, "Invalid encoder identity length");
    return -1;
  }
  for (unsigned index = 0; index < 64; ++index) {
    if (!((bytes[index] >= '0' && bytes[index] <= '9') ||
          (bytes[index] >= 'a' && bytes[index] <= 'f'))) {
      snprintf(error, capacity, "Invalid encoder identity");
      return -1;
    }
  }
  memcpy(destination, bytes, 64);
  destination[64] = '\0';
  return 0;
}

static int publish_metadata(w2c_generator *instance, const char *directory,
                            char *error, size_t capacity) {
  char algorithm[65], fingerprint[65], block[65], metadata[512];
  uint32_t length, candidates;
  const uint8_t *bytes;
  if (identity_result(instance, w2c_generator_mup_lut_identity(instance),
                      algorithm, error, capacity) ||
      identity_result(instance, w2c_generator_mup_lut_patterns(instance),
                      fingerprint, error, capacity) ||
      identity_result(instance, w2c_generator_mup_lut_block_identity(instance),
                      block, error, capacity)) return -1;
  if (w2c_generator_mup_lut_candidate_count(instance)) {
    wasm_error(instance, error, capacity);
    return -1;
  }
  bytes = result(instance, &length);
  if (!bytes || length != 4 || !(candidates = read_u32(bytes))) {
    snprintf(error, capacity, "Invalid candidate count");
    return -1;
  }
  if (w2c_generator_mup_lut_info(instance)) {
    wasm_error(instance, error, capacity);
    return -1;
  }
  bytes = result(instance, &length);
  if (!bytes || length != 16 || read_u32(bytes) != BLOCK_COUNT ||
      read_u32(bytes + 4) != 4096 || read_u32(bytes + 12) != (1u << 24)) {
    snprintf(error, capacity, "Incompatible RGB24 block layout");
    return -1;
  }
  int count = snprintf(metadata, sizeof(metadata),
      "{\n  \"algorithmIdentity\": \"%s\",\n"
      "  \"patternFingerprint\": \"%s\",\n  \"candidateCount\": %u,\n"
      "  \"domain\": 16777216,\n  \"headerBytes\": 128,\n"
      "  \"blockIdentity\": \"%s\"\n}\n",
      algorithm, fingerprint, candidates, block);
  if (count < 0 || (size_t)count >= sizeof(metadata)) {
    snprintf(error, capacity, "Metadata exceeds its buffer");
    return -1;
  }
  char *path = path_for(directory, "lut_metadata.json");
  if (!path) {
    snprintf(error, capacity, "Out of memory reading metadata");
    return -1;
  }
  int file = open(path, O_RDONLY | O_NOFOLLOW);
  free(path);
  if (file >= 0) {
    char previous[sizeof(metadata) + 1];
    size_t received = 0;
    while (received < sizeof(previous)) {
      ssize_t size = read(file, previous + received, sizeof(previous) - received);
      if (size < 0 && errno == EINTR) continue;
      if (size <= 0) break;
      received += (size_t)size;
    }
    close(file);
    if (received == (size_t)count && !memcmp(previous, metadata, received))
      return 0;
    snprintf(error, capacity,
             "Existing metadata differs; choose a fresh output directory");
    return -1;
  }
  if (errno != ENOENT) {
    snprintf(error, capacity, "Cannot read metadata: %s", strerror(errno));
    return -1;
  }
  return publish(directory, "lut_metadata.json", (const uint8_t *)metadata,
                 (size_t)count, error, capacity);
}

static void record_error(Queue *queue, const char *error) {
  pthread_mutex_lock(&queue->mutex);
  if (!queue->error[0]) snprintf(queue->error, sizeof(queue->error), "%s", error);
  pthread_mutex_unlock(&queue->mutex);
}

static void *worker(void *argument) {
  Queue *queue = argument;
  w2c_generator instance;
  char error[512];
  wasm_rt_init_thread();
  if (fesetround(FE_TONEAREST) != 0) {
    record_error(queue, "Cannot select round-to-nearest floating-point mode");
    wasm_rt_free_thread();
    return NULL;
  }
  wasm2c_generator_instantiate(&instance);
  for (;;) {
    unsigned number;
    pthread_mutex_lock(&queue->mutex);
    if (queue->next >= queue->options.end || queue->error[0] || interrupted) {
      pthread_mutex_unlock(&queue->mutex);
      break;
    }
    number = queue->next++;
    pthread_mutex_unlock(&queue->mutex);
    int restored = restore(&instance, queue->options.output, number,
                           error, sizeof(error));
    if (restored < 0) {
      record_error(queue, error);
      break;
    }
    if (!restored) {
      uint32_t length;
      if (w2c_generator_mup_lut_generate(&instance, number)) {
        wasm_error(&instance, error, sizeof(error));
        record_error(queue, error);
        break;
      }
      const uint8_t *bytes = result(&instance, &length);
      if (!bytes || length != BLOCK_BYTES) {
        record_error(queue, "Invalid generated block buffer");
        break;
      }
      char filename[32];
      snprintf(filename, sizeof(filename), "block-%04u.lut", number);
      if (publish(queue->options.output, filename, bytes, length,
                  error, sizeof(error))) {
        record_error(queue, error);
        break;
      }
    }
    pthread_mutex_lock(&queue->mutex);
    queue->completed++;
    queue->restored += (unsigned)restored;
    if (queue->completed % 16 == 0 ||
        queue->completed == queue->options.end - queue->options.start) {
      fprintf(stderr, "RGB24 blocks [%u,%u): %u/%u complete (%u reused), %.1fs.\n",
              queue->options.start, queue->options.end, queue->completed,
              queue->options.end - queue->options.start, queue->restored,
              elapsed(queue));
    }
    pthread_mutex_unlock(&queue->mutex);
  }
  wasm2c_generator_free(&instance);
  wasm_rt_free_thread();
  return NULL;
}

static void help(void) {
  puts("Usage: mupic-lut-generate --output DIRECTORY [--threads auto|N]\n"
       "       [--start BLOCK] [--end BLOCK]\n\n"
       "Generate RGB24 lookup blocks with the shared encoder's C translation.\n"
       "The interval is start-inclusive/end-exclusive, within [0,4096).\n"
       "Completed blocks are validated and reused; metadata is written at start.\n"
       "Full generation can take hours. SIGINT/SIGTERM finish current blocks.");
}

static int parse_number(const char *value, unsigned *number) {
  char *end;
  unsigned long parsed;
  if (!value[0] || value[0] == '-' || value[0] == '+') return -1;
  errno = 0;
  parsed = strtoul(value, &end, 10);
  if (errno || *end || parsed > 0xfffffffful) return -1;
  *number = (unsigned)parsed;
  return 0;
}

int main(int argc, char **argv) {
  long processors = sysconf(_SC_NPROCESSORS_ONLN);
  unsigned automatic = processors > 0 ? (unsigned)processors : 1;
  if (automatic > MAX_THREADS) automatic = MAX_THREADS;
  Options options = { NULL, 0, BLOCK_COUNT, automatic };
  char error[512];
  for (int index = 1; index < argc; ++index) {
    const char *argument = argv[index];
    if (!strcmp(argument, "--help") || !strcmp(argument, "-h")) {
      help();
      return 0;
    }
    if (index + 1 == argc) {
      fprintf(stderr, "Missing value for %s\n", argument);
      return 1;
    }
    const char *value = argv[++index];
    if (!strcmp(argument, "--output")) options.output = value;
    else if (!strcmp(argument, "--threads")) {
      if (!strcmp(value, "auto")) options.threads = automatic;
      else if (parse_number(value, &options.threads)) goto invalid;
    } else if (!strcmp(argument, "--start")) {
      if (parse_number(value, &options.start)) goto invalid;
    } else if (!strcmp(argument, "--end")) {
      if (parse_number(value, &options.end)) goto invalid;
    } else {
      fprintf(stderr, "Unknown option: %s\n", argument);
      return 1;
    }
    continue;
invalid:
    fprintf(stderr, "Invalid value for %s: %s\n", argument, value);
    return 1;
  }
  if (!options.output || !options.output[0] || !options.threads ||
      options.threads > MAX_THREADS || options.start > options.end ||
      options.end > BLOCK_COUNT) {
    fprintf(stderr, "Provide --output and 1..256 threads with 0 <= start <= end <= 4096.\n");
    return 1;
  }
  if (mkdir(options.output, 0700) < 0 && errno != EEXIST) {
    fprintf(stderr, "Cannot create output directory: %s\n", strerror(errno));
    return 1;
  }
  char *lock_path = path_for(options.output, "generation.lock");
  if (!lock_path) return 1;
  int lock = open(lock_path, O_RDWR | O_CREAT | O_NOFOLLOW, 0600);
  free(lock_path);
  if (lock < 0 || flock(lock, LOCK_EX | LOCK_NB) < 0) {
    fprintf(stderr, "Cannot acquire output generation lock: %s\n", strerror(errno));
    if (lock >= 0) close(lock);
    return 1;
  }
  struct sigaction action;
  memset(&action, 0, sizeof(action));
  action.sa_handler = handle_signal;
  sigemptyset(&action.sa_mask);
  sigaction(SIGINT, &action, NULL);
  sigaction(SIGTERM, &action, NULL);
  if (fesetround(FE_TONEAREST) != 0) {
    fprintf(stderr, "Cannot select round-to-nearest floating-point mode\n");
    close(lock);
    return 1;
  }
  wasm_rt_init();
  w2c_generator metadata_instance;
  wasm2c_generator_instantiate(&metadata_instance);
  int metadata_status = publish_metadata(&metadata_instance, options.output,
                                         error, sizeof(error));
  wasm2c_generator_free(&metadata_instance);
  if (metadata_status) {
    fprintf(stderr, "%s\n", error);
    wasm_rt_free();
    close(lock);
    return 1;
  }
  Queue queue = { .options = options, .mutex = PTHREAD_MUTEX_INITIALIZER,
                  .next = options.start };
  clock_gettime(CLOCK_MONOTONIC, &queue.started);
  unsigned total = options.end - options.start;
  unsigned count = options.threads < total ? options.threads : total;
  pthread_t threads[MAX_THREADS];
  pthread_attr_t attributes;
  pthread_attr_init(&attributes);
  int stack_status = pthread_attr_setstacksize(&attributes, 8u * 1024u * 1024u);
  if (stack_status) record_error(&queue, "Cannot configure worker stack size");
  fprintf(stderr, "RGB24 generation [%u,%u): %u CPU workers.\n",
          options.start, options.end, count);
  unsigned started = 0;
  for (; started < count && !stack_status; ++started) {
    int status = pthread_create(&threads[started], &attributes, worker, &queue);
    if (status) {
      snprintf(error, sizeof(error), "Cannot start worker: %s", strerror(status));
      record_error(&queue, error);
      break;
    }
  }
  pthread_attr_destroy(&attributes);
  for (unsigned index = 0; index < started; ++index) pthread_join(threads[index], NULL);
  pthread_mutex_destroy(&queue.mutex);
  wasm_rt_free();
  close(lock);
  if (queue.error[0]) {
    fprintf(stderr, "RGB24 generation failed: %s\n", queue.error);
    return 1;
  }
  if (interrupted) {
    fprintf(stderr, "Stopped after saving %u/%u blocks; rerun to resume.\n",
            queue.completed, total);
    return 128 + interrupted;
  }
  return 0;
}
