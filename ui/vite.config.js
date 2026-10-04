// The site's assets (ADR 0034 §8): Codex's CSS-only components, the default theme's
// typefaces and the site's own styles, built into hashed files with a manifest that
// triplespace-ui reads to name them in each page. The release build embeds ui/dist into
// the binaries that serve the site (rust-embed); Node is a build-time dependency only.
import { defineConfig } from 'vite';

export default defineConfig( {
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
	}
} );
