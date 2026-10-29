import { describe, it, expect, beforeEach } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';
import { apply, applySheets, clean, read, readTheme, resolve, sheetChoices, write, CHOICES, RETIRED, STORAGE_KEY } from './core.js';

const THEME = {
	palette: 'parchment',
	typeset: 'default26',
	palettes: { parchment: '/ui/theme/p-parchment-1.css', encyclopedia: '/ui/theme/p-encyclopedia-2.css' },
	typesets: { default26: '/ui/theme/t-default26-3.css', opendyslexic: '/ui/theme/t-opendyslexic-4.css' }
};

describe( 'appearance', () => {
	beforeEach( () => {
		document.documentElement.className = 'client-js skin-theme-clientpref-os';
		for ( const a of [ 'data-mode', 'data-contrast', 'data-text-size', 'data-vision' ] ) {
			document.documentElement.removeAttribute( a );
		}
		localStorage.clear();
	} );

	it( 'keeps only known keys and values', () => {
		expect( clean( { 'ui.mode': 'dark', 'ui.contrast': 'purple', x: 'y' } ) ).toEqual( { 'ui.mode': 'dark' } );
		expect( clean( null ) ).toEqual( {} );
		expect( clean( 'dark' ) ).toEqual( {} );
	} );

	it( 'writes attributes and the MediaWiki classes', () => {
		const root = document.documentElement;
		apply( root, { 'ui.mode': 'dark', 'ui.contrast': 'more', 'ui.text-size': 'large' } );
		expect( root.getAttribute( 'data-mode' ) ).toBe( 'dark' );
		expect( root.classList.contains( 'skin-theme-clientpref-night' ) ).toBe( true );
		expect( root.classList.contains( 'skin-theme-clientpref-os' ) ).toBe( false );
		expect( root.getAttribute( 'data-contrast' ) ).toBe( 'more' );
		expect( root.getAttribute( 'data-text-size' ) ).toBe( 'large' );
		apply( root, { 'ui.mode': 'auto' } );
		expect( root.hasAttribute( 'data-mode' ) ).toBe( false );
		expect( root.hasAttribute( 'data-contrast' ) ).toBe( false );
		expect( root.classList.contains( 'skin-theme-clientpref-os' ) ).toBe( true );
		expect( root.classList.contains( 'client-js' ) ).toBe( true );
	} );

	it( 'writes the colour-vision alternate', () => {
		const root = document.documentElement;
		apply( root, { 'ui.vision': 'by' } );
		expect( root.getAttribute( 'data-vision' ) ).toBe( 'by' );
		apply( root, { 'ui.vision': 'none' } );
		expect( root.hasAttribute( 'data-vision' ) ).toBe( false );
		apply( root, { 'ui.vision': 'xy' } );
		expect( root.hasAttribute( 'data-vision' ) ).toBe( false );
	} );

	it( 'reads a retired palette as its replacement and alternate', () => {
		expect( resolve( { 'ui.palette': 'parchment-rg', 'ui.mode': 'dark' }, 'parchment' ) )
			.toEqual( { 'ui.palette': '', 'ui.vision': 'rg', 'ui.mode': 'dark' } );
		expect( resolve( { 'ui.palette': 'parchment-by', 'ui.vision': 'rg' }, 'encyclopedia' ) )
			.toEqual( { 'ui.palette': 'parchment', 'ui.vision': 'rg' } );
		expect( resolve( { 'ui.palette': 'encyclopedia' } ) ).toEqual( { 'ui.palette': 'encyclopedia' } );
		localStorage.setItem( STORAGE_KEY, JSON.stringify( { 'ui.palette': 'parchment-by' } ) );
		expect( read( localStorage, { 'ui.palette': [] }, 'parchment' ) ).toEqual( { 'ui.vision': 'by' } );
	} );

	it( 'holds the alternates and retired palettes to the registry', () => {
		const here = dirname( fileURLToPath( import.meta.url ) );

		const reg = parse( readFileSync( join( here, '../../../docs/registry/themes.toml' ), 'utf8' ) );
		expect( CHOICES[ 'ui.vision' ] ).toEqual( [ 'none', ...Object.keys( reg.alternate ).sort() ] );
		const retired = {};
		for ( const [ name, p ] of Object.entries( reg.palette ) ) {
			if ( p.retired && p.replaced_by.some( ( r ) => r.startsWith( 'alternate.' ) ) ) {
				retired[ name ] = [ 'palette.', 'alternate.' ].map( ( pre ) => p.replaced_by.find( ( r ) => r.startsWith( pre ) ).slice( pre.length ) );
			}
		}
		expect( RETIRED ).toEqual( retired );
	} );

	it( 'stores per host and survives refused storage', () => {
		write( localStorage, { 'ui.mode': 'light', bogus: 1 } );
		expect( JSON.parse( localStorage.getItem( STORAGE_KEY ) ) ).toEqual( { 'ui.mode': 'light' } );
		expect( read( localStorage ) ).toEqual( { 'ui.mode': 'light' } );
		const refusing = { getItem() {
			throw new Error( 'denied' );
		}, setItem() {
			throw new Error( 'denied' );
		} };
		expect( read( refusing ) ).toBe( null );
		expect( () => write( refusing, { 'ui.mode': 'dark' } ) ).not.toThrow();
		expect( read( undefined ) ).toBe( null );
	} );

	it( 'keeps a palette or type set only where the wiki offers it', () => {
		const sheets = sheetChoices( THEME );
		expect( sheets ).toEqual( { 'ui.palette': [ 'encyclopedia' ], 'ui.typeset': [ 'opendyslexic' ] } );
		expect( clean( { 'ui.palette': 'encyclopedia', 'ui.typeset': 'comic' }, sheets ) ).toEqual( { 'ui.palette': 'encyclopedia' } );
		expect( clean( { 'ui.palette': 'encyclopedia' } ) ).toEqual( {} );
		write( localStorage, { 'ui.palette': 'encyclopedia', 'ui.mode': 'dark' }, 'browser', sheets );
		expect( read( localStorage, { 'ui.palette': [] } ) ).toEqual( { 'ui.mode': 'dark' } );
		expect( read( localStorage, sheets ) ).toEqual( { 'ui.palette': 'encyclopedia', 'ui.mode': 'dark' } );
	} );

	it( 'points the stylesheet links at the chosen palette and type set', () => {
		document.head.innerHTML = `<script type="application/json" id="ts-theme">${ JSON.stringify( THEME ) }</script>` +
			'<link id="ts-palette" href="/ui/theme/p-parchment-1.css">' +
			'<link id="ts-typeset" href="/ui/theme/t-default26-3.css">';
		const theme = readTheme( document );
		expect( theme ).toEqual( THEME );
		applySheets( document, { 'ui.palette': 'encyclopedia', 'ui.typeset': 'opendyslexic' }, theme );
		expect( document.getElementById( 'ts-palette' ).getAttribute( 'href' ) ).toBe( '/ui/theme/p-encyclopedia-2.css' );
		expect( document.getElementById( 'ts-typeset' ).getAttribute( 'href' ) ).toBe( '/ui/theme/t-opendyslexic-4.css' );
		applySheets( document, { 'ui.palette': '', 'ui.typeset': 'nope' }, theme );
		expect( document.getElementById( 'ts-palette' ).getAttribute( 'href' ) ).toBe( '/ui/theme/p-parchment-1.css' );
		expect( document.getElementById( 'ts-typeset' ).getAttribute( 'href' ) ).toBe( '/ui/theme/t-default26-3.css' );
		document.getElementById( 'ts-theme' ).textContent = '{';
		expect( readTheme( document ) ).toBe( null );
		expect( () => applySheets( document, {}, null ) ).not.toThrow();
		document.head.innerHTML = '';
	} );
} );
