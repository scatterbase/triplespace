// The fallback layers' stylesheets (ADR 0093 §3) are generated from docs/registry/themes.toml
// by scripts/fallbacks.js and checked in, so the Rust build and a reviewer see them; they
// must match what the generator writes from the registry and the packages installed here.
import { describe, it, expect } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { layers, layerCss, layerPath, packageName } from '../scripts/fallbacks.js';

const here = dirname( fileURLToPath( import.meta.url ) );

describe( 'fallback layers', () => {
	const all = Object.entries( layers() );

	it( 'the registry defines one', () => {
		expect( all.map( ( [ name ] ) => name ) ).toContain( 'noto' );
	} );

	for ( const [ name, layer ] of all ) {
		it( `src/fallbacks/${ name }.css is current (npm run fallbacks)`, () => {
			// eslint-disable-next-line security/detect-non-literal-fs-filename
			expect( readFileSync( layerPath( name ), 'utf8' ) ).toBe( layerCss( name, layer ) );
		} );

		it( `every typeface of ${ name } is a dependency`, () => {
			const pkg = JSON.parse( readFileSync( join( here, '../package.json' ), 'utf8' ) );
			for ( const font of layer.font ) {
				expect( Object.keys( pkg.dependencies ) ).toContain( `@fontsource/${ packageName( font.family ) }` );
			}
		} );
	}
} );
