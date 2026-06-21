import * as path from "path";
import { factorize } from "./factorize";

async function main() {
  const input = path.resolve(__dirname, "../fixture/entry.js");

  const build = factorize({ input }, [
    {
      moduleParsed: (id) => console.log("  [plugin] moduleParsed:", path.basename(id)),
      // 모듈 맨 위에 배너 주입
      transform: async (code, id) => `/* transformed: ${path.basename(id)} */\n${code}`,
    },
  ]);

  console.log("building:", input, "\n");
  const out = await build.build();

  console.log(`\nbuilt ${out.modules.length} modules → output:\n`);
  console.log(out.code);
}

main();
