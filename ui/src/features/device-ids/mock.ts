import type { MockModule } from "@/core/mock"
import type { DeviceIdsProfile } from "./api"

const isTrustkernel = () => new URLSearchParams(location.search).get("soc") === "mediatek"
const mock: MockModule = {
  feature: { id: "device-ids", contract: 3 },
  handlers: {
    "device-ids.defaults": () => ({
      backend: isTrustkernel() ? "trustkernel" : "qseecom",
      brand: isTrustkernel() ? "Xelex" : "OnePlus",
      device: isTrustkernel() ? "Q25" : "OP5D55L1",
      product: isTrustkernel() ? "Q25_EEA" : "PJZ110",
      serial: "a1b2c3d4",
      manufacturer: isTrustkernel() ? "Zinwa" : "OnePlus",
      model: isTrustkernel() ? "Q25" : "PJZ110",
      imei: "",
      imei2: "",
      meid: "",
      meid2: "",
      ta_name: isTrustkernel() ? "keymaster" : "keymaster64",
      ta_path: isTrustkernel() ? "/vendor/app/t6" : "/vendor/firmware_mnt/image",
      dry_run: false,
    }),
    "device-ids.provision": (_args, _flags, input) => {
      const profile = input as DeviceIdsProfile
      const trustkernel = isTrustkernel()
      const omitted = trustkernel ? (["meid", "meid2"] as const) : []
      const ids = (
        [
          "brand",
          "device",
          "product",
          "serial",
          "imei",
          "imei2",
          "meid",
          "meid2",
          "manufacturer",
          "model",
        ] as const
      )
        .filter(
          (key) => profile[key].trim() && !(trustkernel && (key === "meid" || key === "meid2")),
        )
        .map((key) => ({ label: key.toUpperCase(), value: profile[key].trim() }))
      return {
        backend: trustkernel ? "trustkernel" : "qseecom",
        verification: profile.dry_run
          ? "not_run"
          : trustkernel
            ? "existence_only"
            : "ta_acknowledgement",
        omitted_ids: omitted.filter((key) => profile[key].trim()).map((key) => key.toUpperCase()),
        count: ids.length,
        ids,
        dry_run: profile.dry_run,
        ta_name: trustkernel ? null : profile.ta_name,
        ta_path: trustkernel ? null : profile.ta_path,
        loaded_library: profile.dry_run
          ? null
          : trustkernel
            ? "/vendor/lib64/libkphproxy.so"
            : "/vendor/lib64/libQSEEComAPI.so",
        executable: null,
        ta_api_version: profile.dry_run || trustkernel ? null : "4.0",
        ta_version: profile.dry_run ? null : trustkernel ? "3.0" : "400",
        report_path: "/data/adb/duck-toolbox/var/outputs/device-ids-20260928.json",
      }
    },
  },
}
export default mock
