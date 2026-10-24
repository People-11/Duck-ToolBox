import { z } from "zod"

import { duckd } from "@/core/duckd"

const profileSchema = z.object({
  brand: z.string(),
  device: z.string(),
  product: z.string(),
  serial: z.string(),
  manufacturer: z.string(),
  model: z.string(),
  imei: z.string(),
  imei2: z.string(),
  meid: z.string(),
  meid2: z.string(),
  ta_name: z.string(),
  ta_path: z.string(),
  dry_run: z.boolean(),
})
export type DeviceIdsProfile = z.infer<typeof profileSchema>
const backendSchema = z.enum(["qseecom", "trustkernel"])
const defaultsSchema = profileSchema.extend({ backend: backendSchema.nullable() })

const resultSchema = z
  .object({
    count: z.number(),
    ids: z.array(z.object({ label: z.string(), value: z.string() })),
    dry_run: z.boolean(),
    backend: backendSchema,
    verification: z.enum(["not_run", "ta_acknowledgement", "existence_only"]),
    omitted_ids: z.array(z.string()),
    ta_name: z.string().nullable(),
    ta_path: z.string().nullable(),
    loaded_library: z.string().nullable(),
    executable: z.string().nullable(),
    ta_api_version: z.string().nullable(),
    ta_version: z.string().nullable(),
    report_path: z.string(),
  })
  .refine((data) => data.count === data.ids.length, {
    message: "ID count disagrees with the reported entries",
  })
  .refine((data) => data.dry_run === (data.verification === "not_run"), {
    message: "Dry run disagrees with verification",
  })
  .refine(
    (data) =>
      data.backend === "trustkernel"
        ? data.loaded_library === null &&
          data.ta_name === null &&
          data.ta_path === null &&
          data.ta_api_version === null &&
          (data.dry_run || (data.executable !== null && data.verification === "existence_only"))
        : data.executable === null &&
          data.ta_name !== null &&
          data.ta_path !== null &&
          data.omitted_ids.length === 0 &&
          (data.dry_run ||
            (data.loaded_library !== null && data.verification === "ta_acknowledgement")),
    { message: "Provisioning metadata disagrees with its backend" },
  )
export type DeviceIdsResult = z.infer<typeof resultSchema>

export const deviceIdsApi = {
  defaults: () => duckd(["device-ids", "defaults"], { schema: defaultsSchema }),
  provision: (profile: DeviceIdsProfile) =>
    duckd(["device-ids", "provision"], { input: profile, schema: resultSchema }),
}
