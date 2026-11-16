// ADR 0034 §9: the components format their messages with banana-i18n, reading the files
// the server reads.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { format } from './format.ts';

/** The shipped English messages, from the file the server embeds. */
const shipped = JSON.parse(

	readFileSync( join( dirname( fileURLToPath( import.meta.url ) ), '../../../i18n/en.json' ), 'utf8' )
);

/** The prefixes of the messages the server hands to a component. */
const HANDED = [ 'ts-edit-', 'ts-sheet-', 'ts-typeahead-', 'ts-bell' ];

/**
 * What the components did before: `$n` replaced by its parameter, nothing else read.
 *
 * @param {string} raw The message.
 * @param {string[]} params The parameters.
 * @return {string} The message filled.
 */
function filled( raw, params ) {
	return raw.replace( /\$(\d+)/g, ( whole, n ) => params[ Number( n ) - 1 ] ?? whole );
}

describe( 'messages in the components', () => {
	it( 'fills parameters and shows a missing key as MediaWiki does', () => {
		const messages = { a: 'Not saved: $1', b: '$2 of $1', c: 'Plain' };
		expect( format( messages, 'en', 'a', [ 'busy' ] ) ).toBe( 'Not saved: busy' );
		expect( format( messages, 'en', 'b', [ 'ten', 'three' ] ) ).toBe( 'three of ten' );
		expect( format( messages, 'en', 'c' ) ).toBe( 'Plain' );
		expect( format( messages, 'en', 'nope' ) ).toBe( '⧼nope⧽' );
		// A parameter is text: it is not read as a message's markup.
		expect( format( messages, 'en', 'a', [ '$1 {{PLURAL:2|x|y}} [[z]] <b>' ] ) )
			.toBe( 'Not saved: $1 {{PLURAL:2|x|y}} [[z]] <b>' );
	} );

	it( 'chooses plural forms by the language, as the server does', () => {
		const messages = { n: '$1 {{PLURAL:$1|change|changes}}' };
		expect( format( messages, 'en', 'n', [ '1' ] ) ).toBe( '1 change' );
		expect( format( messages, 'en', 'n', [ '2' ] ) ).toBe( '2 changes' );
		const ru = { n: '$1 {{PLURAL:$1|правка|правки|правок}}' };
		expect( format( ru, 'ru', 'n', [ '1' ] ) ).toBe( '1 правка' );
		expect( format( ru, 'ru', 'n', [ '3' ] ) ).toBe( '3 правки' );
		expect( format( ru, 'ru', 'n', [ '5' ] ) ).toBe( '5 правок' );
	} );

	it( 'shows a key and its parameters under qqx, and keeps a wikilink as text', () => {
		const qqx = { k: '(k: $1, $2)' };
		expect( format( qqx, 'qqx', 'k', [ 'a', 'b' ] ) ).toBe( '(k: a, b)' );
		expect( format( { l: 'See [[Help:Editing|help]]' }, 'en', 'l' ) )
			.toBe( 'See [[Help:Editing|help]]' );
	} );

	it( 'reads every shipped message a component is handed as it read it before', () => {
		const params = [ 'ONE', 'TWO', 'THREE', 'FOUR' ];
		let checked = 0;
		for ( const prefix of HANDED ) {
			const messages = {};
			for ( const [ key, text ] of Object.entries( shipped ) ) {
				if ( key.startsWith( prefix ) && typeof text === 'string' ) {
					messages[ key ] = text;
				}
			}
			for ( const [ key, text ] of Object.entries( messages ) ) {
				expect( format( messages, 'en', key, params ), key ).toBe( filled( text, params ) );
				checked++;
			}
		}
		expect( checked ).toBeGreaterThan( 100 );
	} );
} );
