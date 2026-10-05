/**
 * What the editing components start from: the JSON block the server writes beside an
 * editable entity's content (ADR 0034 §4), with the interface messages they need.
 */

export interface EditData {
	/** The entity's ID. */
	id: string;
	/** Its type: `item`, `property`. */
	type: string;
	/** Its page title, for `action=render`. */
	title: string;
	/** The interface language, which is also the language terms are edited in. */
	lang: string;
	/** The interface's writing direction. */
	dir: string;
	/** The build of the page, sent with every region request (ADR 0057 §8). */
	build: string;
	/** The `ts-edit-*` messages, unexpanded. */
	messages: Record<string, string>;
}

/**
 * Reads the data block, if the page has one.
 *
 * @param doc The document
 * @return The data, or null on a page with nothing to edit
 */
export function readData( doc: Document ): EditData | null {
	const el = doc.getElementById( 'ts-edit-data' );
	if ( !el || !el.textContent ) {
		return null;
	}
	try {
		return JSON.parse( el.textContent ) as EditData;
	} catch {
		return null;
	}
}

/**
 * A message with `$1`, `$2`, … replaced; `⧼key⧽` when it is missing, as MediaWiki shows
 * one.
 *
 * @param data The edit data
 * @param key The message key
 * @param params The parameters
 * @return The message
 */
export function msg( data: EditData, key: string, ...params: string[] ): string {
	const raw = data.messages[ key ];
	if ( raw === undefined ) {
		return `⧼${ key }⧽`;
	}
	return raw.replace( /\$(\d+)/g, ( whole, n: string ) => {
		const p = params[ Number( n ) - 1 ];
		return p === undefined ? whole : p;
	} );
}
