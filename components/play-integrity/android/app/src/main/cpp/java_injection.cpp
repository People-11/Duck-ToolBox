/* Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
 * Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
 * SPDX-License-Identifier: GPL-3.0-only
 * Modified 2026-10-24: independent engine packaging, bounded configuration and runtime operations.
 */
#include "java_injection.hpp"
#include <android/log.h>
#include <limits>
#define LOGD(...) __android_log_print(ANDROID_LOG_DEBUG, "PIF/Native", __VA_ARGS__)
namespace pif {
jstring javaString(JNIEnv *env, std::string_view text) {
    if (text.size() > static_cast<size_t>(std::numeric_limits<jsize>::max()) || env->PushLocalFrame(5) < 0) return nullptr;
    const auto result = [&]() -> jstring {
        auto bytes = env->NewByteArray(static_cast<jsize>(text.size()));
        if (!bytes || env->ExceptionCheck()) return nullptr;
        env->SetByteArrayRegion(bytes, 0, static_cast<jsize>(text.size()),
                                reinterpret_cast<const jbyte *>(text.data()));
        if (env->ExceptionCheck()) return nullptr;
        auto stringClass = env->FindClass("java/lang/String");
        if (!stringClass || env->ExceptionCheck()) return nullptr;
        auto constructor = env->GetMethodID(stringClass, "<init>", "([BLjava/lang/String;)V");
        if (!constructor || env->ExceptionCheck()) return nullptr;
        auto encoding = env->NewStringUTF("UTF-8");
        if (!encoding || env->ExceptionCheck()) return nullptr;
        auto value = static_cast<jstring>(env->NewObject(stringClass, constructor, bytes, encoding));
        return env->ExceptionCheck() ? nullptr : value;
    }();
    return static_cast<jstring>(env->PopLocalFrame(result));
}

static void setFieldNative(JNIEnv *env, jclass /* clazz_EntryPoint */, jclass targetClass, jobject fieldObj, jstring typeObj, jobject valueObj) {
    if (!targetClass || !fieldObj || !typeObj) return;

    jfieldID fieldID = env->FromReflectedField(fieldObj);
    if (!fieldID) return;

    const char *typeName = env->GetStringUTFChars(typeObj, nullptr);

    if (!typeName) return;

    if (strcmp(typeName, "java.lang.String") == 0) {
        env->SetStaticObjectField(targetClass, fieldID, valueObj);
    } else if (strcmp(typeName, "int") == 0) {
        jclass intClass = env->FindClass("java/lang/Integer");
        if (!intClass || env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        if (!valueObj || !env->IsInstanceOf(valueObj, intClass)) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        jmethodID intValue = env->GetMethodID(intClass, "intValue", "()I");
        if (!intValue || env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        jint val = env->CallIntMethod(valueObj, intValue);
        if (env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        env->SetStaticIntField(targetClass, fieldID, val);
    } else if (strcmp(typeName, "long") == 0) {
        jclass longClass = env->FindClass("java/lang/Long");
        if (!longClass || env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        if (!valueObj || !env->IsInstanceOf(valueObj, longClass)) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        jmethodID longValue = env->GetMethodID(longClass, "longValue", "()J");
        if (!longValue || env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        jlong val = env->CallLongMethod(valueObj, longValue);
        if (env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        env->SetStaticLongField(targetClass, fieldID, val);
    } else if (strcmp(typeName, "boolean") == 0) {
        jclass boolClass = env->FindClass("java/lang/Boolean");
        if (!boolClass || env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        if (!valueObj || !env->IsInstanceOf(valueObj, boolClass)) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        jmethodID booleanValue = env->GetMethodID(boolClass, "booleanValue", "()Z");
        if (!booleanValue || env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        jboolean val = env->CallBooleanMethod(valueObj, booleanValue);
        if (env->ExceptionCheck()) { env->ReleaseStringUTFChars(typeObj, typeName); return; }
        env->SetStaticBooleanField(targetClass, fieldID, val);
    }

    env->ReleaseStringUTFChars(typeObj, typeName);
}

void JavaInjection::initialize(JNIEnv *environment, std::vector<char> dex, const Settings &settings) {
    env=environment; dexVector=std::move(dex); json=settings.build;
    inject();
}
bool JavaInjection::failed(const char *operation) {
        if (!env->ExceptionCheck()) return false;
        LOGD("JNI failed: %s", operation); env->ExceptionDescribe(); env->ExceptionClear(); return true;
    }
void JavaInjection::inject() {
        if (env->PushLocalFrame(32) < 0) { env->ExceptionClear(); return; }
        injectFrame();
        if (env->ExceptionCheck()) { LOGD("DEX injection failed"); env->ExceptionDescribe(); env->ExceptionClear(); }
        env->PopLocalFrame(nullptr);
    }
void JavaInjection::injectFrame() {
        auto clClass = env->FindClass("java/lang/ClassLoader");
        if (!clClass || failed("ClassLoader")) return;
        auto getSystem = env->GetStaticMethodID(clClass, "getSystemClassLoader", "()Ljava/lang/ClassLoader;");
        if (!getSystem || failed("getSystemClassLoader")) return;
        auto systemLoader = env->CallStaticObjectMethod(clClass, getSystem);
        if (!systemLoader || failed("system classloader")) return;
        auto dexClass = env->FindClass("dalvik/system/InMemoryDexClassLoader");
        if (!dexClass || failed("InMemoryDexClassLoader")) return;
        auto init = env->GetMethodID(dexClass, "<init>", "(Ljava/nio/ByteBuffer;Ljava/lang/ClassLoader;)V");
        if (!init || failed("DEX constructor")) return;
        auto buffer = env->NewDirectByteBuffer(dexVector.data(), static_cast<jlong>(dexVector.size()));
        if (!buffer || failed("DEX buffer")) return;
        auto localLoader = env->NewObject(dexClass, init, buffer, systemLoader);
        if (!localLoader || failed("DEX loader")) return;
        loader = env->NewGlobalRef(localLoader);
        if (!loader || failed("loader reference")) return;
        auto load = env->GetMethodID(clClass, "loadClass", "(Ljava/lang/String;)Ljava/lang/Class;");
        if (!load || failed("loadClass method")) return;
        auto className = env->NewStringUTF("es.chiteroman.playintegrityfix.EntryPoint");
        if (!className || failed("entry name")) return;
        auto entry = static_cast<jclass>(env->CallObjectMethod(loader, load, className));
        if (!entry || failed("entry class")) return;
        JNINativeMethod methods[] = {{const_cast<char *>("setFieldNative"), const_cast<char *>("(Ljava/lang/Class;Ljava/lang/reflect/Field;Ljava/lang/String;Ljava/lang/Object;)V"), reinterpret_cast<void *>(setFieldNative)}};
        if (env->RegisterNatives(entry, methods, 1) != JNI_OK || failed("RegisterNatives")) return;
        auto receive = env->GetStaticMethodID(entry, "receiveJson", "(Ljava/lang/String;)V");
        if (!receive || failed("receiveJson")) return;
        auto value = env->NewStringUTF(json.dump(-1, ' ', true).c_str());
        if (!value || failed("config string")) return;
        env->CallStaticVoidMethod(entry, receive, value);
        if (failed("receiveJson call")) return;
        auto method = env->GetStaticMethodID(entry, "init", "()V");
        if (!method || failed("entry init")) return;
        env->CallStaticVoidMethod(entry, method);
        if (!failed("entry init call")) LOGD("Injection initialized for DroidGuard");
    }

}
