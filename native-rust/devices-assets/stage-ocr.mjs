import { createHash, randomUUID } from "node:crypto";
import { mkdir, readFile, writeFile, rename, copyFile, rm } from "node:fs/promises";
import path from "node:path";
if (!process.argv[2]) throw Error("Pass the destination models folder.");
const folder = path.resolve(process.argv[2]);
await mkdir(folder, { recursive: true });
const hashes = {
  "text-detection.onnx": "a917b23dbd9524b465df7e922641b3ff2981623df4ded5a0234004ef2fee7cfe",
  "text-recognition.onnx": "86c145c2edb96c8caed5b1ebb8f44d706408922211451c309c625157dd6061c5",
};
const digest = b => createHash("sha256").update(b).digest("hex");
for (const [name, expected] of Object.entries(hashes)) {
  const target = path.join(folder,name);
  let existing;
  try { existing = await readFile(target); } catch (error) { if (error.code !== "ENOENT") throw error; }
  if (existing && digest(existing) === expected) continue;
  const response = await fetch("https://ocrs-models.s3-accelerate.amazonaws.com/" + name);
  if (!response.ok) throw Error("Could not fetch " + name + " (" + response.status + ")");
  const bytes = Buffer.from(await response.arrayBuffer());
  if (digest(bytes) !== expected) throw Error("The upstream model differs from Buddy's pinned " + name);
  const staged = target + "." + randomUUID() + ".tmp";
  try { await writeFile(staged,bytes,{flag:"wx"}); await rename(staged,target); } finally { await rm(staged,{force:true}); }
}
for (const file of ["OCR-NOTICES.txt","CC-BY-SA-4.0.txt"]) await copyFile(new URL(file,import.meta.url),path.join(folder,file));
