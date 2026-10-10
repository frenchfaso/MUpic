/* Check a translated generator against the shared WASM's RGB/plan fixtures. */
#include "generator.h"
#include <fenv.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint8_t *load(const char *path, size_t *length) {
  FILE *file = fopen(path, "rb");
  uint8_t *bytes = NULL;
  *length = 0;
  if (!file || fseek(file, 0, SEEK_END) || ftell(file) < 0) goto done;
  long size = ftell(file);
  if (size == 0 || size > 16384 || size % 4 || fseek(file, 0, SEEK_SET)) goto done;
  bytes = malloc((size_t)size);
  if (!bytes || fread(bytes, 1, (size_t)size, file) != (size_t)size ||
      fgetc(file) != EOF || ferror(file)) {
    free(bytes);
    bytes = NULL;
    goto done;
  }
  *length = (size_t)size;
done:
  if (file) fclose(file);
  return bytes;
}

int main(int argc, char **argv) {
  size_t keys_length, plans_length;
  if (argc != 3) {
    fprintf(stderr, "Usage: lut-preflight RGB_KEYS.le32 EXPECTED_PLANS.le32\n");
    return 1;
  }
  uint8_t *keys = load(argv[1], &keys_length);
  uint8_t *plans = load(argv[2], &plans_length);
  if (!keys || !plans || keys_length != plans_length) {
    fprintf(stderr, "Cannot read matching nonempty u32LE fixtures (maximum 4096 keys)\n");
    free(keys);
    free(plans);
    return 1;
  }
  if (fesetround(FE_TONEAREST)) {
    fprintf(stderr, "Cannot select round-to-nearest mode\n");
    free(keys);
    free(plans);
    return 1;
  }
  wasm_rt_init();
  w2c_generator instance;
  wasm2c_generator_instantiate(&instance);
  uint32_t pointer = w2c_generator_mup_alloc(&instance, (uint32_t)keys_length);
  wasm_rt_memory_t *memory = w2c_generator_memory(&instance);
  int status = 1;
  if (!pointer || (uint64_t)pointer + keys_length > memory->size) goto done;
  memcpy(memory->data + pointer, keys, keys_length);
  if (w2c_generator_mup_plan_keys(&instance, pointer, (uint32_t)keys_length)) {
    fprintf(stderr, "Encoder rejected the preflight fixture\n");
    goto done;
  }
  uint32_t result = w2c_generator_mup_result_ptr(&instance);
  uint32_t length = w2c_generator_mup_result_len(&instance);
  memory = w2c_generator_memory(&instance);
  if (length != plans_length || (uint64_t)result + length > memory->size ||
      memcmp(memory->data + result, plans, length)) {
    fprintf(stderr, "C/WASM preflight plan mismatch\n");
    goto done;
  }
  printf("C/WASM preflight: %zu/%zu RGB24 plans identical.\n",
         keys_length / 4, keys_length / 4);
  status = 0;
done:
  if (pointer) w2c_generator_mup_free(&instance, pointer, (uint32_t)keys_length);
  wasm2c_generator_free(&instance);
  wasm_rt_free();
  free(keys);
  free(plans);
  return status;
}
