import { z } from "zod"
import { duckd } from "@/core/duckd"
import { previewSchema } from "./api"

const profilesSchema = z.object({
  invalid_profiles: z.array(z.object({ id: z.string(), error: z.string() })).default([]),
  profiles: z.array(
    z.object({ id: z.string(), name: z.string(), created: z.number(), revision: z.string() }),
  ),
})
const resultSchema = z.enum(["unknown", "pass", "fail"])
const recordsSchema = z.object({
  records: z.array(
    z.object({
      id: z.string(),
      created: z.number(),
      app: z.string(),
      version: z.string(),
      note: z.string(),
      results: z.tuple([resultSchema, resultSchema, resultSchema]),
      revision: z.string(),
      model: z.string(),
      fingerprint: z.string(),
      enabled: z.boolean(),
      mode: z.enum(["full", "scripts-only"]),
    }),
  ),
  user_reported: z.literal(true),
  verdict_verified: z.literal(false),
})
export const inspectionSchema = z.object({
  revision: z.string(),
  read_error: z.string().nullable(),
  runtime_observed: z.literal(false),
  verdict_verified: z.literal(false),
  rows: z.array(
    z.object({
      field: z.string(),
      property: z.string(),
      system: z.string().nullable(),
      saved_build: z.string().nullable(),
      saved_property: z.string().nullable(),
      observed: z.null(),
    }),
  ),
})
export type ReportedResult = z.infer<typeof resultSchema>
export type TestRecordInput = {
  app: string
  version: string
  note: string
  basic: ReportedResult
  device: ReportedResult
  strong: ReportedResult
  revision: string
}
const command = (op: string) => ["play-integrity", op]
export const libraryApi = {
  profiles: () => duckd(command("profile-list"), { schema: profilesSchema }),
  saveProfile: (name: string, content: string) =>
    duckd(command("profile-save"), { input: { name, content }, schema: profilesSchema }),
  profilePreview: (id: string) =>
    duckd(command("profile-preview"), { input: { id }, schema: previewSchema }),
  deleteProfile: (id: string) =>
    duckd(command("profile-delete"), { input: { id }, schema: profilesSchema }),
  inspect: () => duckd(command("inspect"), { schema: inspectionSchema }),
  records: () => duckd(command("record-list"), { schema: recordsSchema }),
  saveRecord: (input: TestRecordInput) =>
    duckd(command("record-save"), { input, schema: recordsSchema }),
  clearRecords: () => duckd(command("record-clear"), { schema: recordsSchema }),
}
