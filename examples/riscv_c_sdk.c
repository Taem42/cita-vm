#include <string.h>

#include "pvm.h"
#include "pvm_extend.h"

int main(int argc, char* argv[]) {
  // pvm_debug("Testing: debug");
  // pvm_debug("Test[v]: debug");

  // pvm_debug("Testing: ret");
  // uint8_t *buffer_ret = (uint8_t *)"Test: ret";
  // pvm_ret(&buffer_ret[0], strlen(buffer_ret));
  // pvm_debug("Test[v]: ret");

  // pvm_debug("Testing: save");
  // uint8_t *buffer_save_k = (uint8_t *)"Test: save_k";
  // uint8_t *buffer_save_v = (uint8_t *)"Test: save_v";
  // pvm_save(&buffer_save_k[0], strlen(buffer_save_k), &buffer_save_v[0], strlen(buffer_save_v));
  // pvm_debug("Test[v]: save");

  // pvm_debug("Testing: load");
  // uint8_t buffer_load_v[20];
  // size_t sz;
  // pvm_load(&buffer_save_k[0], strlen(buffer_save_k), &buffer_load_v[0], 20, &sz);
  // const char* s = buffer_load_v;
  // if ((strcmp("Test: save_v", s) == 0) && (sz == 12)) {
  //   pvm_debug("Test[v]: load");
  // } else {
  //   pvm_debug("Test[x]: load");
  // }

  // pvm_debug("Testing: address");
  // uint8_t addr[20];
  // pvm_address(&addr[0]);
  // if (addr[19] == 0x01) {
  //   pvm_debug("Test[v]: address");
  // } else {
  //   pvm_debug("Test[x]: address");
  // }

  // pvm_debug("Testing: balance");
  // uint8_t account1[20] = {
  //   0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
  //   0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
  // };
  // uint8_t v[32];
  // pvm_balance(&account1[0], &v[0]);
  // if (v[31] == 10) {
  //   pvm_debug("Test[v]: balance");
  // } else {
  //   pvm_debug("Test[x]: balance");
  // }

  // pvm_debug("Testing: origin");
  // uint8_t origin[20];
  // pvm_origin(&origin[0]);
  // if (origin[19] == 0x02) {
  //   pvm_debug("Test[v]: origin");
  // } else {
  //   pvm_debug("Test[x]: origin");
  // }

  // pvm_debug("Testing: caller");
  // uint8_t caller[20];
  // pvm_caller(&caller[0]);
  // if (caller[19] == 0x03) {
  //   pvm_debug("Test[v]: caller");
  // } else {
  //   pvm_debug("Test[x]: caller");
  // }

  // pvm_debug("Testing: callvalue");
  // uint8_t callvalue[32];
  // pvm_callvalue(&callvalue[0]);
  // if (callvalue[31] == 5) {
  //   pvm_debug("Test[v]: callvalue");
  // } else {
  //   pvm_debug("Test[x]: callvalue");
  // }

  // pvm_debug("Testing: block hash");
  // uint8_t block_hash[32];
  // pvm_blockhash(7, &block_hash[0]);
  // if (block_hash[31] == 7) {
  //   pvm_debug("Test[v]: block hash");
  // } else {
  //   pvm_debug("Test[x]: block hash");
  // }

  // pvm_debug("Testing: coinbase");
  // uint8_t coinbase[20];
  // pvm_coinbase(&coinbase[0]);
  // if (coinbase[19] == 0x08) {
  //   pvm_debug("Test[v]: coinbase");
  // } else {
  //   pvm_debug("Test[x]: coinbase");
  // }

  // pvm_debug("Testing: timestamp");
  // uint64_t timestamp;
  // pvm_timestamp(&timestamp);
  // if (timestamp == 0x09) {
  //   pvm_debug("Test[v]: timestamp");
  // } else {
  //   pvm_debug("Test[x]: timestamp");
  // }

  // pvm_debug("Testing: number");
  // uint8_t number[32];
  // pvm_number(&number[0]);
  // if (number[31] == 0x06) {
  //   pvm_debug("Test[v]: number");
  // } else {
  //   pvm_debug("Test[x]: number");
  // }

  // pvm_debug("Testing: difficulty");
  // uint8_t difficulty[32];
  // pvm_difficulty(&difficulty[0]);
  // if (difficulty[31] == 0x0a) {
  //   pvm_debug("Test[v]: difficulty");
  // } else {
  //   pvm_debug("Test[x]: difficulty");
  // }

  uint8_t input_amount_hash[32];
  uint8_t output_amount_hash[32];
  uint8_t proof[128];
  pvm_hex2bin("a494fab0b89a5409cdcc4776a128e9100471f971d183f136eebd461fb55e1666", &input_amount_hash[0]);
  pvm_hex2bin("a494fab0b89a5409cdcc4776a128e9100471f971d183f136eebd461fb55e1666", &output_amount_hash[0]);
  pvm_hex2bin("8fadda0f37850349b1f703d453b2f3f5eb5b43a92d502815a2891926dcfa54962e3ce1950f6f34414b6c08a65f58eb4976e1ea83c1449cd625fb4605e456dfce2147dcd11e09fa5c3b06909ca50ab5890530e7295dedd21cab9f1bc9a364f52da444e078100cc13daaf875ebc4e97f10a574985dabd8e13eed1f165c5f04a55a", &proof[0]);
  if (pvm_zk42(&input_amount_hash[0], &output_amount_hash[0], &proof[0], 128) == 0) {
    // success
  } else {
    // failed
  }
  return 0;
}
