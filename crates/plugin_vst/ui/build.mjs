import * as esbuild from "esbuild";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));

async function runBuild() {
  const result = await esbuild.build({
    entryPoints: [path.join(__dirname, "src", "main.tsx")],
    outfile: path.join(__dirname, "app.js"),
    bundle: true,
    format: "iife",
    platform: "browser",
    target: ["es2020"],
    jsx: "automatic",
    minify: false,
    sourcemap: false,
    legalComments: "none",
    define: {
      "process.env.NODE_ENV": '"production"',
    },
    banner: {
      js: "/* TEPUI VST3/CLAP/Standalone React 19 + @audio-ui/react Bundle (Compiled from crates/plugin_vst/ui/src/) */",
    },
  });

  if (result.errors.length > 0) {
    console.error("Build failed:", result.errors);
    process.exit(1);
  }
  console.log("Successfully built crates/plugin_vst/ui/app.js");
}

runBuild().catch((err) => {
  console.error(err);
  process.exit(1);
});
