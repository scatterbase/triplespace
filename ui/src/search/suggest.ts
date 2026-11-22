/**
 * The suggestions of the header's search box (ADR 0034 §4; chapter 19 §2.1), from one
 * call of `GET /suggest` (chapter 3 §11.6): entities by label or alias, a kind at a
 * time, then pages by title, with a "Go to" first when the text names a page.
 */
import type { SearchResult } from '@wikimedia/codex';
import type { Api } from '../edit/api';
import { format } from '../i18n/format';

/** What the server writes beside the box (`ts-search-data`). */
export interface SearchData {
	/** The interface language. */
	lang: string;
	/** The languages the labels are searched in, most wanted first; `lang` alone without. */
	chain?: string[];
	/** The kinds of entity to suggest, in the order they are listed. */
	kinds: string[];
	/** Whether pages are suggested, by title. */
	pages: boolean;
	/** The `ts-typeahead-*` messages, unexpanded. */
	messages: Record<string, string>;
}

/** The most suggestions of one kind. */
export const PER_KIND = 4;

/** The most suggestions in all. */
export const MOST = 10;

/**
 * A message with `$1` replaced; `⧼key⧽` when it is missing, as MediaWiki shows one.
 *
 * @param data The search data
 * @param key The message key
 * @param param The parameter
 * @return The message
 */
export function message( data: SearchData, key: string, param = '' ): string {
	return format( data.messages, data.lang, key, [ param ] );
}

/**
 * The path of a page, as the site links one: spaces as underscores, and the characters
 * a title keeps readable left as they are.
 *
 * @param title The prefixed title
 * @return The path
 */
export function pagePath( title: string ): string {
	return '/wiki/' + encodeURIComponent( title.replace( / /g, '_' ) )
		.replace( /%3A/gi, ':' )
		.replace( /%2F/gi, '/' );
}

/**
 * Where the search page is asked for a text: with `fulltext`, its results; without, the
 * page the text names, if it names one (chapter 19 §2.1).
 *
 * @param query The text
 * @param results Whether to ask for the results whatever the text names
 * @return The URL
 */
export function searchUrl( query: string, results: boolean ): string {
	const q = new URLSearchParams( { title: 'Special:Search', search: query } );
	if ( results ) {
		q.set( 'fulltext', '1' );
	}
	return `/w/index.php?${ q.toString() }`;
}

/**
 * Whether a text is shaped as an entity ID, local or foreign: `Q42`, `P31`, `WDQ42`.
 *
 * @param text The text
 * @return Whether it is
 */
export function looksLikeId( text: string ): boolean {
	return /^[A-Za-z]{0,3}[PQpq][1-9]\d*$/.test( text );
}

interface Text {
	value: string;
}

interface EntityHit {
	id: string;
	title?: string;
	label?: Text;
	description?: Text;
}

interface PageHit {
	title: string;
}

interface Group {
	kind: string;
	hits: ( EntityHit | PageHit )[];
}

/** What `GET /suggest` answers. */
interface Suggestions {
	goto?: { kind: string; id?: string; title: string };
	groups?: Group[];
}

function entity( h: EntityHit ): SearchResult {
	return {
		value: h.id,
		label: h.label?.value ?? h.id,
		description: h.description?.value ?? null,
		supportingText: h.label?.value ? `(${ h.id })` : undefined,
		url: pagePath( h.title ?? h.id ),
	};
}

function page( data: SearchData, h: PageHit ): SearchResult {
	return {
		value: `page:${ h.title }`,
		label: h.title,
		description: message( data, 'ts-typeahead-page' ),
		url: pagePath( h.title ),
	};
}

/**
 * The suggestions for a text: a "Go to" for the page it names, then each kind of
 * entity, then pages. When the call fails, nothing is suggested but a "Go to" for a
 * text shaped as an ID, which the search page resolves.
 *
 * @param api The API client
 * @param data The search data
 * @param text The text typed
 * @return The suggestions, at most `MOST`
 */
export async function suggest( api: Api, data: SearchData, text: string ): Promise<SearchResult[]> {
	const query = text.trim();
	if ( !query ) {
		return [];
	}
	let body: Suggestions = {};
	try {
		body = await api.rest( 'suggest', {
			q: query,
			language: ( data.chain ?? [ data.lang ] ).join( '|' ),
			limit: PER_KIND,
		} ) as Suggestions;
	} catch {
		// Nothing to list; the text may still be an ID.
	}
	const found: SearchResult[] = [];
	const seen = new Set<string | number>();
	for ( const g of body.groups ?? [] ) {
		const pages = g.kind === 'page';
		if ( pages ? !data.pages : !data.kinds.includes( g.kind ) ) {
			continue;
		}
		for ( const h of g.hits ) {
			const r = pages ? page( data, h as PageHit ) : entity( h as EntityHit );
			if ( !seen.has( r.value ) ) {
				seen.add( r.value );
				found.push( r );
			}
		}
	}
	const out = found.slice( 0, MOST );
	// What the text names leads: the hit itself where it is listed, a "Go to" otherwise.
	const go = body.goto;
	const named = go ?
		( go.kind === 'page' ? `page:${ go.title }` : String( go.id ) ) :
		( looksLikeId( query ) ? query.toUpperCase() : undefined );
	if ( named !== undefined ) {
		const key = named.toUpperCase();
		const at = found.findIndex( ( r ) => String( r.value ).toUpperCase() === key );
		if ( at >= 0 ) {
			const lead = found[ at ];
			const rest = out.filter( ( r ) => r !== lead );
			return [ lead, ...rest ].slice( 0, MOST );
		}
		// An entity is named by its ID, as the reader typed it; a page by its title.
		out.unshift( go ? {
			value: `goto:${ go.title }`,
			label: message( data, 'ts-typeahead-goto', go.id ?? go.title ),
			url: pagePath( go.title ),
		} : {
			value: `goto:${ query }`,
			label: message( data, 'ts-typeahead-goto', query ),
			url: searchUrl( query, false ),
		} );
		out.splice( MOST );
	}
	return out;
}
