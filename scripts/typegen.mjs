import { execFileSync } from "node:child_process";
import { writeFileSync, mkdirSync } from "node:fs";
import openapiTS, { astToString } from "openapi-typescript";
const schema = JSON.parse(
  execFileSync(
    "cargo",
    ["run", "-q", "-p", "vsm", "--bin", "vsm", "--", "openapi"],
    { encoding: "utf8", maxBuffer: 10 * 1024 * 1024 },
  ),
);
mkdirSync("docs", { recursive: true });
writeFileSync("docs/openapi.json", JSON.stringify(schema, null, 2) + "\n");
const types = await openapiTS(schema);
writeFileSync("apps/ui/src/schema.d.ts", astToString(types));
