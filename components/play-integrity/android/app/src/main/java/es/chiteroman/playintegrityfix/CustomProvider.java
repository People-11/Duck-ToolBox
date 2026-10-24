/* Derived from Play Integrity Fork, osm0sis, chiteroman and contributors.
 * Source: https://github.com/osm0sis/PlayIntegrityFork/tree/23ec56c2440c4fd1a5e893a23ef7861d6366a828
 * SPDX-License-Identifier: GPL-3.0-only
 * Modified 2026-10-23: independent engine packaging, bounded configuration and runtime operations.
 */
package es.chiteroman.playintegrityfix;

import java.security.Provider;

public final class CustomProvider extends Provider {

    CustomProvider(Provider provider) {
        super(provider.getName(), provider.getVersion(), provider.getInfo());
        putAll(provider);
        this.put("KeyStore.AndroidKeyStore", CustomKeyStoreSpi.class.getName());
    }

    @Override
    public synchronized Service getService(String type, String algorithm) {
        if (EntryPoint.getVerboseLogs() > 2) EntryPoint.LOG(String.format("Service: Caller type '%s' with algorithm '%s'", type, algorithm));
        if (EntryPoint.getSpoofBuildEnabled() > 0) {
            if (type.equals("KeyStore")) EntryPoint.spoofDevice();
        }
        return super.getService(type, algorithm);
    }
}
