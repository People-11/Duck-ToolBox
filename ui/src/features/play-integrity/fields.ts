/** Android's public Build/property names; this is editor metadata, not migration policy. */
export const identityPairs = [
  ["MODEL", "ro.product.model"],
  ["FINGERPRINT", "ro.build.fingerprint"],
  ["SECURITY_PATCH", "ro.build.version.security_patch"],
  ["DEVICE_INITIAL_SDK_INT", "ro.product.first_api_level"],
  ["MANUFACTURER", "ro.product.manufacturer"],
  ["BRAND", "ro.product.brand"],
  ["PRODUCT", "ro.product.name"],
  ["DEVICE", "ro.product.device"],
  ["RELEASE", "ro.build.version.release"],
  ["ID", "ro.build.id"],
  ["INCREMENTAL", "ro.build.version.incremental"],
  ["TYPE", "ro.build.type"],
  ["TAGS", "ro.build.tags"],
] as const

export function pairedDraft(entries: Record<string, string>): Record<string, string> {
  const result = { ...entries }
  for (const [field, property] of identityPairs) {
    if (entries[field]) result[property] = entries[field]
  }
  return result
}
