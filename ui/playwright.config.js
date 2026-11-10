// The end-to-end suite: see e2e/run.sh, which starts what it runs against.
import { defineConfig } from '@playwright/test';

// E2E_CHROMIUM names a Chromium binary to launch instead of the build this Playwright release
// downloads: for a machine that has a browser and cannot download another
// (docs/testing/gotchas.md).
const launchOptions = process.env.E2E_CHROMIUM ?
	{ executablePath: process.env.E2E_CHROMIUM } :
	{};

export default defineConfig( {
	testDir: './e2e',
	fullyParallel: false,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',
	use: {
		baseURL: process.env.E2E_BASE || 'http://127.0.0.1:18100',
		javaScriptEnabled: false
	},
	projects: [ { name: 'chromium', use: { browserName: 'chromium', launchOptions } } ]
} );
