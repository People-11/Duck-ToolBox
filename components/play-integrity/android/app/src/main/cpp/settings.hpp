/* Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
 * Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
 * SPDX-License-Identifier: GPL-3.0-only
 * Modified 2026-10-23: independent engine packaging, bounded configuration and runtime operations.
 */
#pragma once
#include "transport.hpp"
#include <map>
namespace pif {
struct Settings {
    bool vending=false;
    int verboseLogs=0, spoofBuild=1, spoofProps=1, spoofProvider=1, spoofSignature=0;
    int spoofVendingFinger=0, spoofVendingSdk=0;
    std::string vendingFingerprint;
    nlohmann::json build;
    std::map<std::string,std::string> properties;
    bool injectJava() const { return spoofBuild || spoofProvider || spoofSignature || spoofVendingFinger || spoofVendingSdk; }
};
Settings settings(nlohmann::json config, bool vending);
}
