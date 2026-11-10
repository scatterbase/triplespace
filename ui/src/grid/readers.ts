/**
 * Readers for the upload step of `Special:CreateManyEntities` (ADR 0106 §7): each turns
 * a file's contents into a plain table of strings, a header row and the rows below it,
 * which the mapping step then reads as the table's text (0106 §1, §2).
 *
 * - Comma- and tab-separated text, split exactly as the server splits it (`rows_of` in
 *   `crates/triplespace-ui/src/special/create_entities/table.rs`).
 * - JSON Lines: one object per line, its keys the columns.
 * - QuickStatements V1, the subset that creates items: `CREATE` and `LAST` lines.
 * - The first worksheet of an .xlsx workbook, read without a dependency: the ZIP's
 *   central directory by hand, `DecompressionStream` for its entries, and a small reader
 *   for the XML that needs no `DOMParser`.
 */

/** A file as a table: the header row and the rows below it, every cell a trimmed string. */
export interface Sheet {
	kind: 'csv' | 'tsv' | 'xlsx' | 'jsonl' | 'quickstatements';
	headers: string[];
	/** Each row padded or truncated to `headers.length`. */
	rows: string[][];
}

// What Rust's `str::trim` removes (the Unicode White_Space property), so that a cell is
// trimmed here as the server trims it. JavaScript's `trim` differs on U+0085 and U+FEFF.
const WS = '\\t\\n\\v\\f\\r \\u0085\\u00a0\\u1680\\u2000-\\u200a\\u2028\\u2029\\u202f\\u205f\\u3000';
const TRIM = new RegExp( `^[${ WS }]+|[${ WS }]+$`, 'g' );

/**
 * Trims a cell as the server does.
 *
 * @param s The text
 * @return The text without leading and trailing white space
 */
function trim( s: string ): string {
	return s.replace( TRIM, '' );
}

/**
 * The text without a leading byte order mark.
 *
 * @param text The text
 * @return The text
 */
function withoutBom( text: string ): string {
	return text.startsWith( '\ufeff' ) ? text.slice( 1 ) : text;
}

/**
 * The lines of a text, as Rust's `str::lines` gives them: split at `\n`, a `\r` before
 * it dropped.
 *
 * @param text The text
 * @return The lines
 */
function linesOf( text: string ): string[] {
	return text.split( '\n' ).map( ( l ) => ( l.endsWith( '\r' ) ? l.slice( 0, -1 ) : l ) );
}

/**
 * Writes a value as one of several in a cell: a literal bar escaped (`join_values`).
 *
 * @param value The value
 * @return The value, to be joined with `|`
 */
function escapeBar( value: string ): string {
	return value.replace( /\|/g, '\\|' );
}

/**
 * Makes a sheet of rows: the first is the header, the others are padded or truncated to
 * its length, and every cell is trimmed.
 *
 * @param kind What the file was
 * @param rows The rows, the header first
 * @return The sheet
 */
function sheetOf( kind: Sheet[ 'kind' ], rows: string[][] ): Sheet {
	const headers = ( rows[ 0 ] ?? [] ).map( trim );
	return {
		kind,
		headers,
		rows: rows.slice( 1 ).map( ( r ) => headers.map( ( _, i ) => trim( r[ i ] ?? '' ) ) ),
	};
}

/**
 * Splits comma-separated text: double-quoted cells with `""` for a quote, which may
 * hold commas and line breaks. Rows that are entirely empty are skipped.
 *
 * @param text The text, without a byte order mark
 * @return The rows, untrimmed
 */
function commaRows( text: string ): string[][] {
	const rows: string[][] = [];
	let row: string[] = [];
	let cell = '';
	let quoted = false;
	const endRow = (): void => {
		row.push( cell );
		cell = '';
		if ( row.some( ( c ) => trim( c ) !== '' ) ) {
			rows.push( row );
		}
		row = [];
	};
	for ( let i = 0; i < text.length; i++ ) {
		const c = text[ i ];
		if ( quoted ) {
			if ( c !== '"' ) {
				cell += c;
			} else if ( text[ i + 1 ] === '"' ) {
				cell += '"';
				i++;
			} else {
				quoted = false;
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
 * Reads comma- or tab-separated text, as the server reads the table's text: tab-separated
 * when the first non-empty line holds a tab (no quoting), else comma-separated with
 * double-quoted cells. Empty lines are skipped; the first row is the header.
 *
 * @param text The file's text
 * @return The table, of kind `tsv` or `csv`
 */
export function readDelimited( text: string ): Sheet {
	const body = withoutBom( text );
	const lines = linesOf( body ).filter( ( l ) => trim( l ) !== '' );
	if ( ( lines[ 0 ] ?? '' ).includes( '\t' ) ) {
		return sheetOf( 'tsv', lines.map( ( l ) => l.split( '\t' ) ) );
	}
	return sheetOf( 'csv', commaRows( body ) );
}

/**
 * A JSON value as a cell: a scalar as its text, null as nothing, a list as its values
 * joined with `|`, an object as its JSON.
 *
 * @param value The value
 * @param inList Whether the value is one of a list's, where a literal bar is escaped
 * @return The cell's text
 */
function jsonCell( value: unknown, inList = false ): string {
	if ( value === null || value === undefined ) {
		return '';
	}
	if ( Array.isArray( value ) ) {
		if ( inList ) {
			return escapeBar( JSON.stringify( value ) );
		}
		return value.map( ( v ) => jsonCell( v, true ) ).filter( ( v ) => v !== '' ).join( '|' );
	}
	const text = typeof value === 'object' ? JSON.stringify( value ) : String( value );
	return inList ? escapeBar( trim( text ) ) : text;
}

/**
 * Reads JSON Lines: one object per non-empty line. The columns are the objects' keys in
 * the order they are first seen; a list of values becomes one cell, `|`-separated.
 *
 * @param text The file's text
 * @return The table
 * @throws {Error} Naming the line, counted from 1, that is not a JSON object
 */
export function readJsonLines( text: string ): Sheet {
	const headers: string[] = [];
	const index = new Map<string, number>();
	const rows: string[][] = [];
	linesOf( withoutBom( text ) ).forEach( ( line, n ) => {
		if ( trim( line ) === '' ) {
			return;
		}
		let value: unknown;
		try {
			value = JSON.parse( line );
		} catch {
			throw new Error( `Line ${ n + 1 } is not JSON.` );
		}
		if ( typeof value !== 'object' || value === null || Array.isArray( value ) ) {
			throw new Error( `Line ${ n + 1 } is not a JSON object.` );
		}
		const row: string[] = [];
		for ( const [ key, v ] of Object.entries( value ) ) {
			let at = index.get( key );
			if ( at === undefined ) {
				at = headers.length;
				index.set( key, at );
				headers.push( key );
			}
			row[ at ] = jsonCell( v );
		}
		rows.push( row );
	} );
	const sheet = sheetOf( 'jsonl', [ headers, ...rows ] );
	sheet.rows = sheet.rows.filter( ( r ) => r.some( ( c ) => c !== '' ) );
	return sheet;
}

/**
 * The fields of a QuickStatements line: tab-separated when it holds a tab, else
 * `|`-separated, where a bar inside double quotes is not a separator.
 *
 * @param line The line
 * @return The fields, trimmed
 */
function qsFields( line: string ): string[] {
	if ( line.includes( '\t' ) ) {
		return line.split( '\t' ).map( trim );
	}
	const fields: string[] = [];
	let field = '';
	let quoted = false;
	for ( const c of line ) {
		if ( c === '|' && !quoted ) {
			fields.push( field );
			field = '';
			continue;
		}
		if ( c === '"' ) {
			quoted = !quoted;
		}
		field += c;
	}
	fields.push( field );
	return fields.map( trim );
}

/**
 * A QuickStatements string without its double quotes.
 *
 * @param raw The field
 * @return The string
 */
function unquote( raw: string ): string {
	return raw.length >= 2 && raw.startsWith( '"' ) && raw.endsWith( '"' ) ? raw.slice( 1, -1 ) : raw;
}

/**
 * A QuickStatements value as the table writes it: IDs as they are, strings without
 * quotes, times to the day, month or year, quantities without a leading `+`,
 * monolingual texts as `text@lang`, coordinates as `lat, lon`. Anything else is kept
 * as written.
 *
 * @param raw The field
 * @return The cell's value
 */
function qsValue( raw: string ): string {
	if ( raw.length >= 2 && raw.startsWith( '"' ) && raw.endsWith( '"' ) ) {
		return trim( raw.slice( 1, -1 ) );
	}
	const time = /^([+-])(\d+)-(\d\d)-(\d\d)T\d\d:\d\d:\d\dZ\/(\d+)$/.exec( raw );
	if ( time ) {
		const year = ( time[ 1 ] === '-' ? '-' : '' ) + time[ 2 ];
		switch ( time[ 5 ] ) {
			case '11':
				return `${ year }-${ time[ 3 ] }-${ time[ 4 ] }`;
			case '10':
				return `${ year }-${ time[ 3 ] }`;
			case '9':
				return year;
			default:
				return raw;
		}
	}
	const mono = /^([A-Za-z][A-Za-z0-9-]*):"(.*)"$/.exec( raw );
	if ( mono ) {
		return `${ trim( mono[ 2 ] ) }@${ mono[ 1 ] }`;
	}
	const coordinate = /^@\s*(-?\d+(?:\.\d+)?)\s*\/\s*(-?\d+(?:\.\d+)?)$/.exec( raw );
	if ( coordinate ) {
		return `${ coordinate[ 1 ] }, ${ coordinate[ 2 ] }`;
	}
	return /^\+\d/.test( raw ) ? raw.slice( 1 ) : raw;
}

const TERMS: Record<string, string> = { L: 'label', D: 'description', A: 'aliases' };

/**
 * Reads QuickStatements V1 commands that create items: each `CREATE` starts a row, and
 * the `LAST` lines after it fill the row's labels, descriptions, aliases, statements
 * and their references' values. Qualifiers have no place in the table and are left out.
 *
 * @param text The file's text
 * @param lang The wiki's language, whose terms' columns are written without a suffix
 * @return The table, or null when the text is not this subset of QuickStatements: no
 *   `CREATE`, or a line that is neither `CREATE` nor a `LAST` line this reader knows
 */
export function readQuickStatements( text: string, lang: string ): Sheet | null {
	const headers: string[] = [];
	const index = new Map<string, number>();
	const rows: string[][] = [];
	let row: string[] | null = null;
	const put = ( into: string[], header: string, value: string, several: boolean ): void => {
		let at = index.get( header );
		if ( at === undefined ) {
			at = headers.length;
			index.set( header, at );
			headers.push( header );
		}
		if ( !several ) {
			into[ at ] = value;
		} else if ( value !== '' ) {
			into[ at ] = into[ at ] ? `${ into[ at ] }|${ escapeBar( value ) }` : escapeBar( value );
		}
	};
	for ( const full of linesOf( withoutBom( text ) ) ) {
		const line = trim( full.replace( /\/\*.*?\*\//g, '' ) );
		if ( line === '' || line.startsWith( '#' ) ) {
			continue;
		}
		const fields = qsFields( line );
		while ( fields.length > 0 && fields[ fields.length - 1 ] === '' ) {
			fields.pop();
		}
		if ( fields.length === 1 && fields[ 0 ] === 'CREATE' ) {
			row = [];
			rows.push( row );
			continue;
		}
		if ( fields[ 0 ] !== 'LAST' || row === null || fields.length < 3 ) {
			return null;
		}
		const term = /^([LDA])([a-z]{2,}(?:-[a-z0-9]+)*)$/.exec( fields[ 1 ] );
		if ( term ) {
			if ( fields.length !== 3 ) {
				return null;
			}
			const name = TERMS[ term[ 1 ] ];
			const header = term[ 2 ] === lang ? name : `${ name }:${ term[ 2 ] }`;
			put( row, header, trim( unquote( fields[ 2 ] ) ), term[ 1 ] === 'A' );
			continue;
		}
		if ( !/^P\d+$/i.test( fields[ 1 ] ) || fields.length % 2 === 0 ) {
			return null;
		}
		const property = fields[ 1 ].toUpperCase();
		put( row, property, qsValue( fields[ 2 ] ), true );
		for ( let i = 3; i < fields.length; i += 2 ) {
			const source = /^S(\d+)$/i.exec( fields[ i ] );
			if ( source ) {
				put( row, `${ property }/ref:P${ source[ 1 ] }`, qsValue( fields[ i + 1 ] ), true );
			} else if ( !/^P\d+$/i.test( fields[ i ] ) ) {
				return null;
			}
		}
	}
	if ( rows.length === 0 ) {
		return null;
	}
	const sheet = sheetOf( 'quickstatements', [ headers, ...rows ] );
	sheet.rows = sheet.rows.filter( ( r ) => r.some( ( c ) => c !== '' ) );
	return sheet;
}

/** A file in a ZIP's central directory. */
interface ZipEntry {
	/** 0 for stored, 8 for deflated. */
	method: number;
	/** Where its local header starts. */
	offset: number;
	/** The length of its data as stored. */
	size: number;
}

/**
 * Reads a ZIP's central directory.
 *
 * @param bytes The file
 * @return The entries by name
 * @throws {Error} When the bytes are not a ZIP file
 */
function zipEntries( bytes: Uint8Array ): Map<string, ZipEntry> {
	const view = new DataView( bytes.buffer, bytes.byteOffset, bytes.byteLength );
	// The end record is the last 22 bytes, or sits before a comment of up to 65535.
	let end = -1;
	for ( let i = bytes.length - 22; i >= 0 && i >= bytes.length - 22 - 0xffff; i-- ) {
		if ( view.getUint32( i, true ) === 0x06054b50 ) {
			end = i;
			break;
		}
	}
	if ( end < 0 ) {
		throw new Error( 'This is not an .xlsx workbook: it is not a ZIP file.' );
	}
	const count = view.getUint16( end + 10, true );
	let at = view.getUint32( end + 16, true );
	const entries = new Map<string, ZipEntry>();
	const decoder = new TextDecoder();
	for ( let n = 0; n < count; n++ ) {
		if ( at + 46 > bytes.length || view.getUint32( at, true ) !== 0x02014b50 ) {
			throw new Error( 'This .xlsx workbook cannot be read: its ZIP directory is damaged.' );
		}
		const nameLength = view.getUint16( at + 28, true );
		const name = decoder.decode( bytes.subarray( at + 46, at + 46 + nameLength ) );
		entries.set( name.replace( /^\/+/, '' ), {
			method: view.getUint16( at + 10, true ),
			size: view.getUint32( at + 20, true ),
			offset: view.getUint32( at + 42, true ),
		} );
		at += 46 + nameLength + view.getUint16( at + 30, true ) + view.getUint16( at + 32, true );
	}
	return entries;
}

/**
 * Inflates raw deflate data with the platform's `DecompressionStream`.
 *
 * @param data The deflated bytes
 * @return The bytes
 */
async function inflate( data: Uint8Array ): Promise<Uint8Array> {
	const stream = new DecompressionStream( 'deflate-raw' );
	const writer = stream.writable.getWriter();
	// Written while the other end is read, so a large entry does not stall; a failure
	// shows on the reading side, so here it only has to be caught.
	const written = writer.write( new Uint8Array( data ) )
		.then( () => writer.close() )
		.catch( () => undefined );
	const reader = stream.readable.getReader();
	const chunks: Uint8Array[] = [];
	let length = 0;
	for ( ;; ) {
		const { done, value } = await reader.read();
		if ( done ) {
			break;
		}
		chunks.push( value );
		length += value.length;
	}
	await written;
	const out = new Uint8Array( length );
	let at = 0;
	for ( const chunk of chunks ) {
		out.set( chunk, at );
		at += chunk.length;
	}
	return out;
}

/**
 * The text of a file in a ZIP.
 *
 * @param bytes The ZIP file
 * @param entries Its central directory
 * @param name The file's name
 * @return The file's text, or null when the ZIP has no such file
 * @throws {Error} When the file cannot be read
 */
async function zipText(
	bytes: Uint8Array, entries: Map<string, ZipEntry>, name: string,
): Promise<string | null> {
	const entry = entries.get( name );
	if ( !entry ) {
		return null;
	}
	const damaged = new Error( `This .xlsx workbook cannot be read: ${ name } is damaged.` );
	const view = new DataView( bytes.buffer, bytes.byteOffset, bytes.byteLength );
	if ( entry.offset + 30 > bytes.length || view.getUint32( entry.offset, true ) !== 0x04034b50 ) {
		throw damaged;
	}
	const start = entry.offset + 30 + view.getUint16( entry.offset + 26, true ) +
		view.getUint16( entry.offset + 28, true );
	if ( start + entry.size > bytes.length ) {
		throw damaged;
	}
	const data = bytes.subarray( start, start + entry.size );
	if ( entry.method === 0 ) {
		return new TextDecoder().decode( data );
	}
	if ( entry.method !== 8 ) {
		throw new Error( `This .xlsx workbook cannot be read: ${ name } is compressed in a way that is not supported.` );
	}
	try {
		return new TextDecoder().decode( await inflate( data ) );
	} catch {
		throw damaged;
	}
}

const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'" };

/**
 * Decodes XML's five named entities and its numeric character references.
 *
 * @param text The text as written in the XML
 * @return The text
 */
function decodeXml( text: string ): string {
	return text.replace( /&(#x[0-9a-fA-F]+|#\d+|amp|lt|gt|quot|apos);/g, ( whole, name: string ) => {
		if ( !name.startsWith( '#' ) ) {
			return ENTITIES[ name ];
		}
		const code = name[ 1 ] === 'x' ? parseInt( name.slice( 2 ), 16 ) : parseInt( name.slice( 1 ), 10 );
		return code <= 0x10ffff ? String.fromCodePoint( code ) : whole;
	} );
}

/** An element found by {@link elements}. */
interface Element {
	/** Its attributes, as written. */
	attrs: string;
	/** What it contains, as written; empty for an empty element. */
	inner: string;
}

/**
 * The elements of a name in a piece of XML, whatever their namespace prefix. Good for
 * elements that do not nest in themselves, which is all a workbook needs.
 *
 * @param xml The XML
 * @param name The element's local name
 * @return The elements, in order
 */
function elements( xml: string, name: string ): Element[] {
	const re = new RegExp(
		`<(?:[\\w.-]+:)?${ name }(?=[\\s/>])([^>]*?)(?:/>|>([\\s\\S]*?)</(?:[\\w.-]+:)?${ name }\\s*>)`, 'g',
	);
	return Array.from( xml.matchAll( re ), ( m ) => ( { attrs: m[ 1 ], inner: m[ 2 ] ?? '' } ) );
}

/**
 * An attribute's value.
 *
 * @param attrs An element's attributes, as written
 * @param name The attribute's name, as a regular expression
 * @return The value, or null when the element has no such attribute
 */
function attr( attrs: string, name: string ): string | null {
	const m = new RegExp( `(?:^|\\s)${ name }\\s*=\\s*(?:"([^"]*)"|'([^']*)')` ).exec( attrs );
	return m ? decodeXml( m[ 1 ] ?? m[ 2 ] ) : null;
}

/**
 * The text of a string item (`<si>`, `<is>`): its `<t>` runs together, without the
 * phonetic readings.
 *
 * @param xml What the item contains
 * @return The text
 */
function runsText( xml: string ): string {
	const plain = xml.replace( /<(?:[\w.-]+:)?rPh[\s>][\s\S]*?<\/(?:[\w.-]+:)?rPh\s*>/g, '' );
	return elements( plain, 't' ).map( ( t ) => decodeXml( t.inner ) ).join( '' );
}

/**
 * The path in the ZIP of a workbook's first sheet.
 *
 * @param workbook `xl/workbook.xml`, if the ZIP has it
 * @param rels `xl/_rels/workbook.xml.rels`, if the ZIP has it
 * @return The path, or null when the two do not say
 */
function firstSheetPath( workbook: string | null, rels: string | null ): string | null {
	if ( workbook === null || rels === null ) {
		return null;
	}
	const sheet = elements( workbook, 'sheet' )[ 0 ];
	const id = sheet ? attr( sheet.attrs, '[\\w.-]+:id' ) : null;
	const rel = elements( rels, 'Relationship' ).find( ( r ) => attr( r.attrs, 'Id' ) === id );
	const target = rel ? attr( rel.attrs, 'Target' ) : null;
	if ( id === null || !target ) {
		return null;
	}
	// A target is relative to xl/, or absolute in the package.
	const parts: string[] = [];
	for ( const part of ( target.startsWith( '/' ) ? target : `xl/${ target }` ).split( '/' ) ) {
		if ( part === '..' ) {
			parts.pop();
		} else if ( part !== '' && part !== '.' ) {
			parts.push( part );
		}
	}
	return parts.join( '/' );
}

/** The last column a worksheet can have (XFD), counted from 0. */
const LAST_COLUMN = 16383;

/**
 * The column of a cell reference, counted from 0: `A1` is 0, `AB7` is 27.
 *
 * @param ref The reference
 * @return The column, or null when the reference names none
 */
function columnOf( ref: string | null ): number | null {
	const letters = /^\$?([A-Za-z]+)/.exec( ref ?? '' );
	if ( !letters ) {
		return null;
	}
	let n = 0;
	for ( const c of letters[ 1 ].toUpperCase() ) {
		n = n * 26 + c.charCodeAt( 0 ) - 64;
	}
	return n - 1;
}

/**
 * A cell's value as text.
 *
 * @param cell The `<c>` element
 * @param shared The workbook's shared strings
 * @return The text
 */
function cellText( cell: Element, shared: string[] ): string {
	const type = attr( cell.attrs, 't' );
	if ( type === 'inlineStr' ) {
		return elements( cell.inner, 'is' ).map( ( is ) => runsText( is.inner ) ).join( '' );
	}
	const v = elements( cell.inner, 'v' )[ 0 ];
	const value = v ? decodeXml( v.inner ) : '';
	if ( type === 's' ) {
		return shared[ Number( trim( value ) ) ] ?? '';
	}
	if ( ( type === null || type === 'n' ) && /[.eE]/.test( value ) && Number.isFinite( Number( value ) ) ) {
		// A number as it is shown rather than as the binary fraction stored:
		// 0.1 for 0.10000000000000001.
		return String( Number( value ) );
	}
	return value;
}

/**
 * Reads the first worksheet of an .xlsx workbook: shared and inline strings, and other
 * cells as the values stored (a date is its serial number). Cells are placed by their
 * references, so a gap stays an empty cell; rows that are entirely empty are skipped,
 * and the first of the others is the header.
 *
 * @param bytes The file
 * @return The table
 * @throws {Error} When the bytes are not a workbook that can be read
 */
export async function readXlsx( bytes: ArrayBuffer ): Promise<Sheet> {
	const zip = new Uint8Array( bytes );
	const entries = zipEntries( zip );
	const path = firstSheetPath(
		await zipText( zip, entries, 'xl/workbook.xml' ),
		await zipText( zip, entries, 'xl/_rels/workbook.xml.rels' ),
	);
	const sheetXml = ( path === null ? null : await zipText( zip, entries, path ) ) ??
		await zipText( zip, entries, 'xl/worksheets/sheet1.xml' );
	if ( sheetXml === null ) {
		throw new Error( 'This is not an .xlsx workbook: it has no worksheet.' );
	}
	const sharedXml = await zipText( zip, entries, 'xl/sharedStrings.xml' );
	const shared = sharedXml === null ? [] : elements( sharedXml, 'si' ).map( ( si ) => runsText( si.inner ) );

	const rows: string[][] = [];
	for ( const rowElement of elements( sheetXml, 'row' ) ) {
		const row: string[] = [];
		let next = 0;
		for ( const cell of elements( rowElement.inner, 'c' ) ) {
			const column = columnOf( attr( cell.attrs, 'r' ) ) ?? next;
			next = column + 1;
			const text = trim( cellText( cell, shared ) );
			if ( text !== '' && column <= LAST_COLUMN ) {
				row[ column ] = text;
			}
		}
		if ( row.length > 0 ) {
			// A gap is an empty cell, not a hole.
			rows.push( Array.from( row, ( c ) => c ?? '' ) );
		}
	}
	return sheetOf( 'xlsx', rows );
}

/**
 * Reads an uploaded file as a table, choosing the reader by the file's extension
 * (`.xlsx`; `.jsonl`, `.ndjson`; `.qs`) and otherwise by its contents: QuickStatements
 * when it reads as that, JSON Lines when its first line opens an object, else comma- or
 * tab-separated text.
 *
 * @param file The file
 * @param lang The wiki's language ({@link readQuickStatements})
 * @return The table
 * @throws {Error} When the file cannot be read as its kind, or holds no header row
 */
export async function readFile( file: File, lang: string ): Promise<Sheet> {
	const dot = file.name.lastIndexOf( '.' );
	const extension = dot < 0 ? '' : file.name.slice( dot + 1 ).toLowerCase();
	let sheet: Sheet | null;
	if ( extension === 'xlsx' ) {
		sheet = await readXlsx( await file.arrayBuffer() );
	} else {
		const text = await file.text();
		if ( extension === 'jsonl' || extension === 'ndjson' ) {
			sheet = readJsonLines( text );
		} else if ( extension === 'qs' ) {
			sheet = readQuickStatements( text, lang );
			if ( sheet === null ) {
				throw new Error(
					'This file is not QuickStatements that create items: only CREATE and LAST lines can be read.',
				);
			}
		} else {
			const first = linesOf( withoutBom( text ) ).map( trim ).find( ( l ) => l !== '' ) ?? '';
			sheet = readQuickStatements( text, lang ) ??
				( first.startsWith( '{' ) ? readJsonLines( text ) : readDelimited( text ) );
		}
	}
	if ( sheet.headers.length === 0 ) {
		throw new Error( 'This file is empty: it has no header row.' );
	}
	return sheet;
}
