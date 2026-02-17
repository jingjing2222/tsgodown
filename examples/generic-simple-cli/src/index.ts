function parseArgs(argv: string[]) {
  const nameArg = argv.find((arg) => arg.startsWith("--name="));
  const name = nameArg ? nameArg.slice("--name=".length) : "world";
  const shout = argv.includes("--shout");
  return { name, shout };
}

function renderGreeting(name: string, shout: boolean) {
  const text = `hello, ${name}`;
  return shout ? text.toUpperCase() : text;
}

const { name, shout } = parseArgs(process.argv.slice(2));
console.log(renderGreeting(name, shout));
