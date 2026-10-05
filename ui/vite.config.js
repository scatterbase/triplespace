// The site's assets (ADR 0034 §8): Codex's CSS-only components, the default theme's
// typefaces, the site's own styles and the editing components (Vue, loaded lazily),
// built into hashed files with a manifest that triplespace-ui reads to name them in each
// page. The release build embeds ui/dist into
// the binaries that serve the site (rust-embed); Node is a build-time dependency only.
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

export default defineConfig( {
	plugins: [ vue() ],
	base: '/ui/assets/',
	build: {
		outDir: 'dist',
		emptyOutDir: true,
		manifest: 'manifest.json',
		assetsDir: '',
		cssCodeSplit: true,
		rollupOptions: {
			input: {
				site: 'src/site.css',
				'site-rtl': 'src/site-rtl.css',
				main: 'src/main.js'
			}
		}
	},
	test: {
		// Vitest's unit tests live beside the code; e2e/ is Playwright's (npm run e2e).
		include: [ 'src/**/*.{test,spec}.{js,ts}' ],
		environment: 'happy-dom'
	}
} );
