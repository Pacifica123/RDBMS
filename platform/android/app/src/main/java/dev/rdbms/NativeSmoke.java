package dev.rdbms;

/** Minimal Java-side JNI smoke wrapper for Stage 10. */
public final class NativeSmoke {
    static {
        System.loadLibrary("rdbms_android");
    }

    private NativeSmoke() {
    }

    public static native int stage();

    public static native int abiVersion();

    public static native int add(int left, int right);
}
