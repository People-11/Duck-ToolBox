/* Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
 * Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
 * SPDX-License-Identifier: GPL-3.0-only
 * Modified 2026-10-24: independent engine packaging, bounded operations and typed Build values.
 * Modified 2026-10-26: Build spoofing only; no provider, signature or verbose-log paths.
 */
package es.chiteroman.playintegrityfix;

import android.os.Build;
import android.util.JsonReader;
import android.util.Log;

import org.lsposed.hiddenapibypass.HiddenApiBypass;

import java.io.IOException;
import java.io.StringReader;
import java.lang.reflect.Field;
import java.util.HashMap;
import java.util.Map;

public final class EntryPoint {
    private static final Map<String, String> map = new HashMap<>();

    public static void init() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            HiddenApiBypass.addHiddenApiExemptions("");
        }
        spoofDevice();
    }

    public static void receiveJson(String data) {
        try (JsonReader reader = new JsonReader(new StringReader(data))) {
            reader.beginObject();
            while (reader.hasNext()) {
                map.put(reader.nextName(), reader.nextString());
            }
            reader.endObject();
        } catch (IOException|IllegalStateException e) {
            LOG("Couldn't read JSON from Zygisk: " + e);
            map.clear();
            return;
        }
    }

    static void spoofDevice() {
        for (String key : map.keySet()) {
            try { setField(key, map.get(key)); }
            catch (RuntimeException e) { LOG("Invalid field " + key + ": " + e); }
        }
    }

    private static boolean classContainsField(Class className, String fieldName) {
        for (Field field : className.getDeclaredFields()) {
            if (field.getName().equals(fieldName)) return true;
        }
        return false;
    }

    private static void setField(String name, String value) {
        if (value.isEmpty()) {
            LOG(String.format("%s is empty, skipping", name));
            return;
        }

        Field field = null;
        String oldValue = null;
        Object newValue = null;

        try {
            if (classContainsField(Build.class, name)) {
                field = Build.class.getDeclaredField(name);
            } else if (classContainsField(Build.VERSION.class, name)) {
                field = Build.VERSION.class.getDeclaredField(name);
            } else {
                return;
            }
        } catch (NoSuchFieldException e) {
            LOG(String.format("Couldn't find '%s' field name: " + e, name));
            return;
        }
        field.setAccessible(true);
        try {
            Field accessFlagsField = Field.class.getDeclaredField("accessFlags");
            accessFlagsField.setAccessible(true);
            accessFlagsField.setInt(field, field.getModifiers() & ~java.lang.reflect.Modifier.FINAL);
        } catch (NoSuchFieldException | IllegalAccessException e) {
            LOG(String.format("Couldn't modify accessFlags for '%s': " + e, name));
        }
        try {
            oldValue = String.valueOf(field.get(null));
        } catch (IllegalAccessException e) {
            LOG(String.format("Couldn't access '%s' field value: " + e, name));
            return;
        }
        if (value.equals(oldValue)) return;
        Class<?> fieldType = field.getType();
        newValue = FieldValue.parse(fieldType, value);
        try {
            setFieldNative(field.getDeclaringClass(), field, fieldType.getName(), newValue);
        } catch (Exception e) {
            LOG(String.format("Native setField failed for '%s': " + e, name));
        }
        LOG(String.format("[%s]: %s -> %s", name, oldValue, value));
    }

    private static native void setFieldNative(Class<?> targetClass, Field field, String type, Object value);

    static void LOG(String msg) {
        Log.d("PIF/Java:DG", msg);
    }
}
