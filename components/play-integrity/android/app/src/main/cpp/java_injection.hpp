/* Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
 * Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
 * SPDX-License-Identifier: GPL-3.0-only
 * Modified 2026-10-24: independent injection and explicit standard UTF-8 conversion.
 * Modified 2026-10-26: DroidGuard entry only, with fixed spoofing switches.
 */
#pragma once
#include <jni.h>
#include <string_view>
#include "settings.hpp"
namespace pif {
// Configuration text is standard UTF-8, while NewStringUTF expects Modified UTF-8.
jstring javaString(JNIEnv *env, std::string_view text);
class JavaInjection {
public:
    void initialize(JNIEnv *environment, std::vector<char> dex, const Settings &settings);
private:
    JNIEnv *env = nullptr;
    std::vector<char> dexVector;
    nlohmann::json json;
    jobject loader = nullptr;
    bool failed(const char *operation);
    void inject();
    void injectFrame();
};
}
