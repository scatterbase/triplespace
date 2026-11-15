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
 * An API that answers from a table keyed by entity type, and `pages` for prefixsearch.
 *
 * @param answers The answers
 * @param asked Collects each query asked
 * @return The API
 */
function api( answers: Record<string, unknown>, asked: string[] = [] ): Api {
	return new Api( ( input ) => {
		const q = new URL( input, 'https://wiki.example' ).searchParams;
		asked.push( q.toString() );
		const key = q.get( 'action' ) === 'wbsearchentities' ? String( q.get( 'type' ) ) : 'pages';
		const body = answers[ key ];
		if ( body === 'fail' ) {
			return Promise.resolve( new Response( 'no', { status: 500 } ) );
		}
		return Promise.resolve( new Response( JSON.stringify( body ?? {} ) ) );
	} );
}

/**
 * A `wbsearchentities` hit.
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
		display: {
			...( label ? { label: { value: label } } : {} ),
			...( description ? { description: { value: description } } : {} ),
		},
	};
}

describe( 'the header suggestions', () => {
	it( 'lists entities a kind at a time, then pages, each with its link', async () => {
		const asked: string[] = [];
		const found = await suggest( api( {
			item: { search: [ hit( 'Q6', 'Item:Q6', 'Six & more', 'a number' ) ] },
			property: { search: [ hit( 'P2', 'Property:P2', 'instance of' ) ] },
			domain: { search: [ hit( 'domain:six.example', 'Domain:six.example' ) ] },
			pages: { query: { prefixsearch: [ { title: 'Help:Six pages/Intro' } ] } },
		}, asked ), data, ' six ' );
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
		// The text is trimmed, searched in the interface language, one call a kind.
		expect( asked ).toHaveLength( 4 );
		expect( asked[ 0 ] ).toContain( 'search=six&language=en&type=item' );
		expect( asked[ 3 ] ).toContain( 'list=prefixsearch&pssearch=six&psnamespace=*' );
	} );

	it( 'offers Go to first for an ID nothing names, and leads with the entity it names', async () => {
		const none = await suggest( api( {} ), data, 'q42' );
		expect( none ).toEqual( [ {
			value: 'goto:q42',
			label: 'Go to q42',
			url: '/w/index.php?title=Special%3ASearch&search=q42',
		} ] );
		const named = await suggest( api( {
			item: { search: [ hit( 'Q60', 'Item:Q60', 'Sixty' ), hit( 'Q6', 'Item:Q6', 'Six' ) ] },
		} ), data, 'Q6' );
		expect( named.map( ( r ) => r.value ) ).toEqual( [ 'Q6', 'Q60' ] );
	} );

	it( 'leaves out a kind whose call fails, asks nothing for no text, and caps the list', async () => {
		const some = await suggest( api( {
			item: 'fail',
			property: { search: [ hit( 'P2', 'Property:P2', 'instance of' ) ] },
		} ), data, 'inst' );
		expect( some.map( ( r ) => r.value ) ).toEqual( [ 'P2' ] );
		const asked: string[] = [];
		expect( await suggest( api( {}, asked ), data, '   ' ) ).toEqual( [] );
		expect( asked ).toEqual( [] );
		const many = ( type: string ): unknown => ( {
			search: [ 1, 2, 3, 4 ].map( ( n ) => hit( `${ type }${ n }`, `T:${ type }${ n }` ) ),
		} );
		const capped = await suggest( api( {
			item: many( 'Q' ), property: many( 'P' ), domain: many( 'D' ),
		} ), { ...data, pages: false }, 'Q9' );
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
