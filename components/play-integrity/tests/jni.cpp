// SPDX-License-Identifier: GPL-3.0-only
// Exercise the production UTF-8 boundary with a real host JVM and checked JNI calls.
#include "java_injection.hpp"
#include <cassert>
#include <string>

int __android_log_print(int, const char *, const char *, ...) { return 0; }

static void check(JNIEnv *env, std::string_view input, std::u16string_view expected) {
    auto value = pif::javaString(env, input);
    assert(value && !env->ExceptionCheck());
    assert(env->GetStringLength(value) == static_cast<jsize>(expected.size()));
    const auto *characters = env->GetStringChars(value, nullptr);
    assert(characters && !env->ExceptionCheck());
    for (size_t i = 0; i < expected.size(); ++i) assert(characters[i] == expected[i]);
    env->ReleaseStringChars(value, characters);
    env->DeleteLocalRef(value);
}

int main() {
    JavaVM *vm = nullptr;
    JNIEnv *env = nullptr;
    JavaVMOption checked{const_cast<char *>("-Xcheck:jni"), nullptr};
    JavaVMInitArgs args{};
    args.version = JNI_VERSION_1_8;
    args.nOptions = 1;
    args.options = &checked;
    assert(JNI_CreateJavaVM(&vm, reinterpret_cast<void **>(&env), &args) == JNI_OK);
    check(env, "", u"");
    check(env, "google/device:16/BUILD:user/release-keys", u"google/device:16/BUILD:user/release-keys");
    check(env, "\xe4\xb8\xad\xe6\x96\x87", u"\u4e2d\u6587");
    check(env, "A\xf0\x9f\x98\x80Z", u"A\U0001f600Z");
    check(env, std::string("a\0b", 3), std::u16string(u"a\0b", 3));
    for (int i = 0; i < 1000; ++i) check(env, "\xf0\x9f\x98\x80", u"\U0001f600");
    assert(vm->DestroyJavaVM() == JNI_OK);
}
