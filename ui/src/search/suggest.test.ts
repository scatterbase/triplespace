// ADR 0034 §4; chapter 19 §2.1: the header's suggestions.
import { describe, expect, it } from 'vitest';
import { Api } from '../edit/api';
import { MOST, looksLikeId, message, pagePath, searchUrl, suggest } from './suggest';
import type { SearchData } from './suggest';

const data: SearchData = {
	lang: 'en',
	kinds: [ 'item', 'property', 'domain' ],
	pages: true,
	messages: {
		'ts-typeahead-goto': 'Go to $1',
		'ts-typeahead-footer': 'Search for “$1”',
		'ts-typeahead-page': 'Page',
	},
};

/**
 * An API that answers `GET /suggest` with a body, or fails.
 *
 * @param body The answer; `'fail'` for a failure
 * @param asked Collects each URL asked
 * @return The API
 */
function api( body: unknown, asked: string[] = [] ): Api {
	return new Api( ( input ) => {
		asked.push( input );
		if ( body === 'fail' ) {
			return Promise.resolve( new Response( 'no', { status: 500 } ) );
		}
		return Promise.resolve( new Response( JSON.stringify( body ) ) );
	} );
}

/**
 * An entity hit of `GET /suggest`.
 *
 * @param id The entity's ID
 * @param title Its page title
 * @param label Its label, if it has one
 * @param description Its description, if it has one
 * @return The hit
 */
function hit( id: string, title: string, label?: string, description?: string ): unknown {
	return {
		id,
		title,
		...( label ? { label: { language: 'en', value: label } } : {} ),
		...( description ? { description: { language: 'en', value: description } } : {} ),
	};
}

describe( 'the header suggestions', () => {
	it( 'lists entities a kind at a time, then pages, each with its link', async () => {
		const asked: string[] = [];
		const found = await suggest( api( { groups: [
			{ kind: 'item', hits: [ hit( 'Q6', 'Item:Q6', 'Six & more', 'a number' ) ] },
			{ kind: 'property', hits: [ hit( 'P2', 'Property:P2', 'instance of' ) ] },
			{ kind: 'domain', hits: [ hit( 'domain:six.example', 'Domain:six.example' ) ] },
			{ kind: 'page', hits: [ { title: 'Help:Six pages/Intro', ns: 12, pageid: 3 } ] },
		] }, asked ), { ...data, chain: [ 'de', 'mul', 'en' ] }, ' six ' );
		expect( found.map( ( r ) => [ r.value, r.label, r.url ] ) ).toEqual( [
			[ 'Q6', 'Six & more', '/wiki/Item:Q6' ],
			[ 'P2', 'instance of', '/wiki/Property:P2' ],
			[ 'domain:six.example', 'domain:six.example', '/wiki/Domain:six.example' ],
			[ 'page:Help:Six pages/Intro', 'Help:Six pages/Intro', '/wiki/Help:Six_pages/Intro' ],
		] );
		expect( found[ 0 ].description ).toBe( 'a number' );
		expect( found[ 0 ].supportingText ).toBe( '(Q6)' );
		expect( found[ 2 ].supportingText ).toBeUndefined();
		expect( found[ 3 ].description ).toBe( 'Page' );
		// One call: the text trimmed, searched along the chain the server wrote.
		expect( asked ).toEqual( [
			'/w/rest.php/triplespace/v0/suggest?q=six&language=de%7Cmul%7Cen&limit=4',
		] );
	} );

	it( 'leads with what the text names: the hit itself, or a Go to', async () => {
		// An entity the server found by its ID, which no label matched.
		const go = await suggest( api( {
			goto: { kind: 'entity', id: 'Q42', title: 'Item:Q42' },
			groups: [ { kind: 'item', hits: [ hit( 'Q7', 'Item:Q7', 'Q42 the film' ) ] } ],
		} ), data, 'q42' );
		expect( go.map( ( r ) => [ r.value, r.label, r.url ] ) ).toEqual( [
			[ 'goto:Item:Q42', 'Go to Item:Q42', '/wiki/Item:Q42' ],
			[ 'Q7', 'Q42 the film', '/wiki/Item:Q7' ],
		] );
		// One that is also listed leads the list, once.
		const named = await suggest( api( {
			goto: { kind: 'entity', id: 'Q6', title: 'Item:Q6' },
			groups: [ { kind: 'item', hits: [ hit( 'Q60', 'Item:Q60', 'Sixty' ), hit( 'Q6', 'Item:Q6', 'Six' ) ] } ],
		} ), data, 'Q6' );
		expect( named.map( ( r ) => r.value ) ).toEqual( [ 'Q6', 'Q60' ] );
		// A page by its exact title.
		const titled = await suggest( api( {
			goto: { kind: 'page', title: 'Help:Six' },
			groups: [ { kind: 'page', hits: [ { title: 'Help:Sixty' }, { title: 'Help:Six' } ] } ],
		} ), data, 'help:six' );
		expect( titled.map( ( r ) => r.value ) ).toEqual( [ 'page:Help:Six', 'page:Help:Sixty' ] );
		// A text shaped as an ID that the server names nothing for is the search page's
		// to resolve.
		const none = await suggest( api( { groups: [] } ), data, 'q42' );
		expect( none ).toEqual( [ {
			value: 'goto:q42',
			label: 'Go to q42',
			url: '/w/index.php?title=Special%3ASearch&search=q42',
		} ] );
	} );

	it( 'lists the kinds the site offers, asks nothing for no text, and caps the list', async () => {
		const groups = [
			{ kind: 'item', hits: [ hit( 'Q6', 'Item:Q6', 'Six' ) ] },
			{ kind: 'lexeme', hits: [ hit( 'L6', 'Lexeme:L6', 'six' ) ] },
			{ kind: 'page', hits: [ { title: 'Six' } ] },
		];
		const some = await suggest( api( { groups } ), { ...data, pages: false }, 'six' );
		expect( some.map( ( r ) => r.value ) ).toEqual( [ 'Q6' ] );
		// A failed call suggests nothing, but for an ID.
		expect( await suggest( api( 'fail' ), data, 'six' ) ).toEqual( [] );
		expect( ( await suggest( api( 'fail' ), data, 'Q9' ) ).map( ( r ) => r.value ) )
			.toEqual( [ 'goto:Q9' ] );
		const asked: string[] = [];
		expect( await suggest( api( {}, asked ), data, '   ' ) ).toEqual( [] );
		expect( asked ).toEqual( [] );
		const many = ( type: string ): unknown => ( {
			kind: type,
			hits: [ 1, 2, 3, 4 ].map( ( n ) => hit( `${ type }${ n }`, `T:${ type }${ n }` ) ),
		} );
		const capped = await suggest( api( {
			groups: [ many( 'item' ), many( 'property' ), many( 'domain' ) ],
		} ), data, 'Q9' );
		expect( capped ).toHaveLength( MOST );
		expect( capped[ 0 ].value ).toBe( 'goto:Q9' );
	} );

	it( 'words its messages and its links', () => {
		expect( message( data, 'ts-typeahead-footer', 'a$1b' ) ).toBe( 'Search for “a$1b”' );
		expect( message( data, 'ts-missing' ) ).toBe( '⧼ts-missing⧽' );
		expect( pagePath( 'Item talk:Q6' ) ).toBe( '/wiki/Item_talk:Q6' );
		expect( pagePath( 'A?b#c' ) ).toBe( '/wiki/A%3Fb%23c' );
		expect( searchUrl( 'six & more', true ) )
			.toBe( '/w/index.php?title=Special%3ASearch&search=six+%26+more&fulltext=1' );
		expect( looksLikeId( 'WDQ42' ) ).toBe( true );
		expect( looksLikeId( 'Q0' ) ).toBe( false );
		expect( looksLikeId( 'Quark' ) ).toBe( false );
	} );
} );
