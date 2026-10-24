// SPDX-License-Identifier: GPL-3.0-only
// Play Integrity Fork derivative, revision 23ec56c; modified 2026-10-24:
// bounded, ABI-independent transport with a shared monotonic exchange deadline.
#pragma once
#include <cstdint>
#include <chrono>
#include <vector>
#include <string>
#include "json/single_include/nlohmann/json.hpp"
namespace pif {
constexpr uint32_t magic = 0x50494631;
constexpr uint32_t max_dex = 16 * 1024 * 1024;
constexpr uint32_t max_config = 64 * 1024;
using Deadline = std::chrono::steady_clock::time_point;
bool transfer(int fd, void *buffer, size_t size, bool writing,
              Deadline deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2));
bool receive(int fd, std::vector<char> &dex, std::string &config);
void companion(int fd);
nlohmann::json parse(const std::string &raw);
int number(const nlohmann::json &value, int fallback);
}
