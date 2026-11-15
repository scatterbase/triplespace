// ADR 0077 §3: the built JavaScript and CSS carry the licence header.
import { describe, expect, it } from 'vitest';
import { LICENCE_HEADER, licenceHeader, takesHeader, withHeader } from './licence-header.js';

describe( 'the licence header', () => {
	it( 'names the licence and Special:Version in a comment minifiers keep', () => {
		expect( LICENCE_HEADER.startsWith( '/*!' ) ).toBe( true );
		expect( LICENCE_HEADER ).toContain( '@license GPL-3.0-or-later' );
		expect( LICENCE_HEADER ).toContain( 'Special:Version' );
		expect( LICENCE_HEADER.endsWith( '*/\n' ) ).toBe( true );
	} );

	it( 'goes on JavaScript and CSS, and on nothing else', () => {
		expect( takesHeader( 'main-abc.js' ) ).toBe( true );
		expect( takesHeader( 'site-abc.css' ) ).toBe( true );
		expect( takesHeader( 'inter-abc.woff2' ) ).toBe( false );
		expect( takesHeader( 'manifest.json' ) ).toBe( false );
		expect( takesHeader( 'packages.json' ) ).toBe( false );
	} );

	it( 'is added once to each script and stylesheet the build wrote', async () => {
		const files = {
			'/out/main-1.js': 'const __vite__mapDeps=1;export{};',
			'/out/site-1.css': 'a{color:red}',
			'/out/font-1.woff2': 'binary',
			'/out/manifest.json': '{}'
		};
		const writes = [];
		const fs = {
			readFile: ( path ) => Promise.resolve( files[ path ] ),
			writeFile: ( path, text ) => {
				files[ path ] = text;
				writes.push( path );
				return Promise.resolve();
			}
		};
		const bundle = Object.fromEntries(
			Object.keys( files ).map( ( p ) => [ p, { fileName: p.replace( '/out/', '' ) } ] )
		);
		const plugin = licenceHeader( fs );
		await plugin.writeBundle( { dir: '/out' }, bundle );
		await plugin.writeBundle( { dir: '/out' }, bundle );
		expect( files[ '/out/main-1.js' ] ).toBe(
			LICENCE_HEADER + 'const __vite__mapDeps=1;export{};'
		);
		expect( files[ '/out/site-1.css' ] ).toBe( LICENCE_HEADER + 'a{color:red}' );
		expect( files[ '/out/font-1.woff2' ] ).toBe( 'binary' );
		expect( files[ '/out/manifest.json' ] ).toBe( '{}' );
		expect( writes ).toEqual( [ '/out/main-1.js', '/out/site-1.css' ] );
		expect( withHeader( withHeader( 'x' ) ) ).toBe( LICENCE_HEADER + 'x' );
	} );
} );
