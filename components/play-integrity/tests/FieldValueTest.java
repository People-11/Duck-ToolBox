// SPDX-License-Identifier: GPL-3.0-only
// Build value conversion regression tests; no Android runtime or native library is needed.
package es.chiteroman.playintegrityfix;

public final class FieldValueTest {
    private static void check(Class<?> type, String value, Object expected) {
        Object actual = FieldValue.parse(type, value);
        if (!expected.equals(actual) || actual.getClass() != expected.getClass()) {
            throw new AssertionError(type + ": " + value + " -> " + actual);
        }
    }

    private static void reject(Class<?> type, String value) {
        try {
            FieldValue.parse(type, value);
        } catch (IllegalArgumentException expected) {
            return;
        }
        throw new AssertionError("Accepted invalid " + type + ": " + value);
    }

    public static void main(String[] args) {
        switch (args[0]) {
            case "booleans":
                for (String value : new String[] {"true", "TRUE", "1"}) {
                    check(boolean.class, value, Boolean.TRUE);
                }
                for (String value : new String[] {"false", "FALSE", "0"}) {
                    check(boolean.class, value, Boolean.FALSE);
                }
                break;
            case "numbers":
                check(int.class, "2147483647", Integer.MAX_VALUE);
                check(int.class, "-2147483648", Integer.MIN_VALUE);
                check(long.class, "9223372036854775807", Long.MAX_VALUE);
                check(long.class, "-9223372036854775808", Long.MIN_VALUE);
                break;
            case "strings":
                for (String value : new String[] {"true", "1", "0", "a#b", "https://a/b"}) {
                    check(String.class, value, value);
                }
                break;
            case "invalid":
                for (String value : new String[] {"yes", "2", "", " true "}) {
                    reject(boolean.class, value);
                }
                reject(int.class, "2147483648");
                reject(long.class, "9223372036854775808");
                reject(int.class, "invalid");
                reject(String[].class, "a,b");
                break;
            default:
                throw new AssertionError("Unknown test case");
        }
    }
}
