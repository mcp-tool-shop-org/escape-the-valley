#!/usr/bin/env node
"use strict";

const { version: wrapperVersion } = require("../package.json");

const arg = process.argv[2];
if (arg === "--version" || arg === "-V") {
  console.log(`escape-the-valley (npm wrapper) ${wrapperVersion}`);
  process.exit(0);
}

// version/tag refer to the source repo binary release, not the npm wrapper version.
// Update these when a new GitHub Release is published for escape-the-valley.
process.env.MCPTOOLSHOP_LAUNCH_CONFIG = JSON.stringify({
  toolName: "escape-the-valley",
  owner: "mcp-tool-shop-org",
  repo: "escape-the-valley",
  version: "1.1.0",
  tag: "v1.1.0",
});

require("@mcptoolshop/npm-launcher/bin/mcptoolshop-launch.js");
