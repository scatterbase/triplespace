// The end-to-end suite: see e2e/run.sh, which starts what it runs against.
import { defineConfig } from '@playwright/test';

export default defineConfig( {
	testDir: './e2e',
	fullyParallel: false,
	workers: 1,
	reporter: process.env.CI ? 'github' : 'list',
	use: {
		baseURL: process.env.E2E_BASE || 'http://127.0.0.1:18100',
		javaScriptEnabled: false
	},
	projects: [ { name: 'chromium', use: { browserName: 'chromium' } } ]
} );
