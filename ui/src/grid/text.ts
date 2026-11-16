/**
 * The table's text (ADR 0106 §1, §2; 21 §6.6), in the browser: the same reading and
 * writing as `create_entities/table.rs` does on the server, so that the grid edits the
 * text the page posts and nothing else. The two are held to the same vectors in their
 * tests.
 */

/** What a column holds. */
export type ColumnKind =
	'id' | 'label' | 'description' | 'aliases' | 'property' | 'new' | 'reference' | 'plain';

/** A column, as its header names it. */
export interface Column {
	kind: ColumnKind;
	/** A term's language. */
	language?: string;
	/** A property's ID; for a reference, the reference snak's property. */
	property?: string;
	/** For a reference: the property whose statements carry it. */
	of?: string;
	/** A new property's label, or a plain column's header. */
	label?: string;
	/** A new property's data type, where the header gives one. */
	datatype?: string;
}

const PROPERTY = /^[A-Za-z]{0,3}[Pp]\d+$/;
const LANGUAGE = /^[a-z]{2,8}(-[a-z0-9]{1,8})*$/;

/**
 * Whether text is a property ID.
 *
 * @param id The text
 * @return Whether it is
 */
export function isProperty( id: string ): boolean {
	return PROPERTY.test( id.trim() );
}

/**
 * Reads a header cell.
 *
 * @param header The header as written
 * @param lang The wiki's language, which a term without one is in
 * @return The column
 */
export function column( header: string, lang: string ): Column {
	const h = header.trim();
	const lower = h.toLowerCase();
	if ( lower === 'id' ) {
		return { kind: 'id' };
	}
	if ( lower.startsWith( 'new:' ) ) {
		const rest = h.slice( 4 );
		const at = rest.lastIndexOf( '@' );
		const datatype = at >= 0 ? rest.slice( at + 1 ).trim() : '';
		if ( at >= 0 && datatype !== '' && !/\s/.test( datatype ) && rest.slice( 0, at ).trim() !== '' ) {
			return { kind: 'new', label: rest.slice( 0, at ).trim(), datatype };
		}
		return rest.trim() === '' ? { kind: 'plain', label: h } : { kind: 'new', label: rest.trim() };
	}
	const ref = h.indexOf( '/ref:' );
	if ( ref >= 0 ) {
		const of = h.slice( 0, ref ).trim();
		const property = h.slice( ref + 5 ).trim();
		if ( isProperty( of ) && isProperty( property ) ) {
			return { kind: 'reference', of: of.toUpperCase(), property: property.toUpperCase() };
		}
		return { kind: 'plain', label: h };
	}
	const colon = lower.indexOf( ':' );
	const name = ( colon >= 0 ? lower.slice( 0, colon ) : lower ).trim();
	const given = colon >= 0 ? lower.slice( colon + 1 ).trim() : '';
	const language = given === '' ? lang : ( LANGUAGE.test( given ) ? given : null );
	if ( language !== null ) {
		if ( name === 'label' || name === 'labels' ) {
			return { kind: 'label', language };
		}
		if ( name === 'description' || name === 'descriptions' ) {
			return { kind: 'description', language };
		}
		if ( name === 'alias' || name === 'aliases' ) {
			return { kind: 'aliases', language };
		}
	}
	if ( isProperty( h ) ) {
		return { kind: 'property', property: h.toUpperCase() };
	}
	return { kind: 'plain', label: h };
}

/**
 * Writes a column as its header: what `column` reads back as the same column.
 *
 * @param c The column
 * @param lang The wiki's language
 * @return The header
 */
export function headerOf( c: Column, lang: string ): string {
	const term = ( name: string ): string => ( c.language === lang || !c.language ? name : `${ name }:${ c.language }` );
	switch ( c.kind ) {
		case 'id':
			return 'id';
		case 'label':
			return term( 'label' );
		case 'description':
			return term( 'description' );
		case 'aliases':
			return term( 'aliases' );
		case 'property':
			return c.property ?? '';
		case 'new':
			return c.datatype ? `new:${ c.label ?? '' }@${ c.datatype }` : `new:${ c.label ?? '' }`;
		case 'reference':
			return `${ c.of ?? '' }/ref:${ c.property ?? '' }`;
		default:
			return c.label ?? '';
	}
}

// Without the byte-order mark a spreadsheet's export may begin with.
function unmarked( text: string ): string {
	return text.charCodeAt( 0 ) === 0xFEFF ? text.slice( 1 ) : text;
}

function firstLine( text: string ): string {
	return text.split( '\n' ).find( ( l ) => l.trim() !== '' ) ?? '';
}

/**
 * Whether text is read as tab-separated.
 *
 * @param text The table's text
 * @return Whether its first line holds a tab
 */
export function isTabbed( text: string ): boolean {
	return firstLine( unmarked( text ) ).includes( '\t' );
}

/**
 * Splits a table: tab-separated when the first line holds a tab, else comma-separated
 * with double-quoted cells as a spreadsheet writes them. Empty lines are skipped.
 *
 * @param source The table's text
 * @return The rows, the header first, every cell trimmed
 */
export function rowsOf( source: string ): string[][] {
	const text = unmarked( source );
	if ( isTabbed( text ) ) {
		return text.split( '\n' )
			.filter( ( l ) => l.trim() !== '' )
			.map( ( l ) => l.replace( /\r$/, '' ).split( '\t' ).map( ( c ) => c.trim() ) );
	}
	const rows: string[][] = [];
	let row: string[] = [];
	let cell = '';
	let quoted = false;
	const endRow = (): void => {
		row.push( cell );
		cell = '';
		if ( row.some( ( c ) => c.trim() !== '' ) ) {
			rows.push( row.map( ( c ) => c.trim() ) );
		}
		row = [];
	};
	for ( let i = 0; i < text.length; i++ ) {
		const c = text[ i ];
		if ( quoted ) {
			if ( c === '"' ) {
				if ( text[ i + 1 ] === '"' ) {
					cell += '"';
					i++;
				} else {
					quoted = false;
				}
			} else {
				cell += c;
			}
		} else if ( c === '"' && cell === '' ) {
			quoted = true;
		} else if ( c === ',' ) {
			row.push( cell );
			cell = '';
		} else if ( c === '\n' ) {
			endRow();
		} else if ( c !== '\r' ) {
			cell += c;
		}
	}
	endRow();
	return rows;
}

/**
 * Writes a table as text that `rowsOf` reads back as the same rows: tab-separated when
 * asked and the cells allow it, else comma-separated with the cells that need it
 * quoted.
 *
 * @param rows The rows, the header first
 * @param tabbed Whether tabs are wanted
 * @return The text
 */
export function textOf( rows: string[][], tabbed: boolean ): string {
	const width = Math.max( 0, ...rows.map( ( r ) => r.length ) );
	const plain = ( c: string ): boolean => !/[\t\n\r]/.test( c );
	const tabs = tabbed && width > 1 && rows.every( ( r ) => r.length > 1 && r.every( plain ) );
	return rows.map( ( r ) => r.map( ( c ) => {
		if ( tabs || !( /[,"\n\r]/.test( c ) || c.startsWith( ' ' ) ) ) {
			return c;
		}
		return `"${ c.replace( /"/g, '""' ) }"`;
	} ).join( tabs ? '\t' : ',' ) + '\n' ).join( '' );
}

/**
 * The values of a cell that may hold several: `|`-separated, `\|` a literal bar and
 * `\\` a literal backslash.
 *
 * @param cell The cell
 * @return The values, trimmed, the empty ones dropped
 */
export function splitValues( cell: string ): string[] {
	const out: string[] = [];
	let cur = '';
	for ( let i = 0; i < cell.length; i++ ) {
		if ( cell[ i ] === '\\' && ( cell[ i + 1 ] === '|' || cell[ i + 1 ] === '\\' ) ) {
			// An escaped bar or backslash is itself; any other backslash is as written.
			cur += cell[ i + 1 ];
			i++;
		} else if ( cell[ i ] === '|' ) {
			out.push( cur );
			cur = '';
		} else {
			cur += cell[ i ];
		}
	}
	out.push( cur );
	return out.map( ( v ) => v.trim() ).filter( ( v ) => v !== '' );
}

/**
 * Writes several values as one cell, which `splitValues` reads back.
 *
 * @param values The values
 * @return The cell
 */
export function joinValues( values: string[] ): string {
	return values.map( ( v ) => v.replace( /[\\|]/g, '\\$&' ) ).join( '|' );
}

/**
 * The row a cell value names with `row:{n}`.
 *
 * @param value The value
 * @return The row's number, from 1, or null
 */
export function rowRef( value: string ): number | null {
	const m = /^row:\s*(\d+)$/i.exec( value.trim() );
	return m && Number( m[ 1 ] ) > 0 ? Number( m[ 1 ] ) : null;
}
