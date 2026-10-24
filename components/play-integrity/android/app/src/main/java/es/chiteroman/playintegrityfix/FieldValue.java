// SPDX-License-Identifier: GPL-3.0-only
// Modified 2026-10-24: preserve normalized JSON booleans when assigning Build fields.
package es.chiteroman.playintegrityfix;

final class FieldValue {
    private FieldValue() {}

    static Object parse(Class<?> type, String value) {
        if (type == String.class) return value;
        if (type == int.class) return Integer.valueOf(value);
        if (type == long.class) return Long.valueOf(value);
        if (type == boolean.class) {
            if (value.equals("1") || value.equalsIgnoreCase("true")) return Boolean.TRUE;
            if (value.equals("0") || value.equalsIgnoreCase("false")) return Boolean.FALSE;
            throw new IllegalArgumentException("Expected true, false, 1 or 0");
        }
        throw new IllegalArgumentException("Unsupported Build field type: " + type.getName());
    }
}
