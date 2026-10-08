// The site's assets (ADR 0034 §8): Codex's CSS-only components, the default theme's
// typefaces, the site's own styles and the editing components (Vue, loaded lazily),
// built into hashed files with a manifest that triplespace-ui reads to name them in each
// page. The release build embeds ui/dist into
// the binaries that serve the site (rust-embed); Node is a build-time dependency only.
import { defineConfig } from 'vitest/config';
import vue from '@vitejs/plugin-vue';

/**
 * The npm packages whose files end up in the build, as `packages.json` beside the
 * manifest: the frontend packages `cargo xtask manifest` lists on Special:Version with
 * their licences (ADR 0077 §6, §15). A package that is only a build tool, such as Vue's
 * compiler, is not in it.
 *
 * @return {Object} The plugin.
 */
function bundledPackages() {
	const nameOf = ( id ) => {
		const path = id.replace( /\\/g, '/' );
		const at = path.lastIndexOf( 'node_modules/' );
		if ( at < 0 ) {
			return null;
		}
		const parts = path.slice( at + 'node_modules/'.length ).split( '/' );
		return parts[ 0 ].startsWith( '@' ) ? parts.slice( 0, 2 ).join( '/' ) : parts[ 0 ];
	};
	return {
		name: 'triplespace-bundled-packages',
		generateBundle( options, bundle ) {
			const names = new Set();
			for ( const item of Object.values( bundle ) ) {
				let ids;
				if ( item.type === 'chunk' ) {
					ids = item.moduleIds || Object.keys( item.modules || {} );
				} else if ( item.originalFileNames ) {
					ids = item.originalFileNames;
				} else {
					ids = item.originalFileName ? [ item.originalFileName ] : [];
				}
				for ( const id of ids ) {
					const name = nameOf( id );
					if ( name ) {
						names.add( name );
					}
				}
			}
			this.emitFile( {
				type: 'asset',
				fileName: 'packages.json',
				source: JSON.stringify( [ ...names ].sort(), null, '\t' ) + '\n'
			} );
		}
	};
}

export default defineConfig( {
	plugins: [ vue(), bundledPackages() ],
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
