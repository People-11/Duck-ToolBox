/* Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
 * Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
 * SPDX-License-Identifier: GPL-3.0-only
 * Modified 2026-10-23: independent engine packaging, bounded configuration and runtime operations.
 * Modified 2026-10-26: fixed spoofBuild/spoofProps on, provider, signature, vending and verbose
 * logging off; only DroidGuard is injected.
 */
#pragma once
#include "transport.hpp"
#include <map>
namespace pif {
struct Settings {
    nlohmann::json build;
    std::map<std::string,std::string> properties;
};
Settings settings(const nlohmann::json &config);
}
