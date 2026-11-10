/**
 * What the grid components start from: the JSON block the server writes on
 * `Special:CreateManyEntities` and on a Table page the viewer may edit (ADR 0034 §4;
 * 0106 §1; 0045 §6), with the interface messages they need.
 */

/** What both pages' blocks hold. */
interface Common {
	/** The wiki's language, which a term column without one is in. */
	lang: string;
	/** The interface's writing direction. */
	dir: string;
	/** The base of the site's concept IRIs, which a quantity's unit is written under. */
	conceptBase: string;
	/** The `ts-sheet-*` messages, unexpanded. */
	messages: Record<string, string>;
}

/** `Special:CreateManyEntities`. */
export interface CreateData extends Common {
	page: 'create';
	/** Whether the viewer may create properties, and so declare one in a column. */
	canCreateProperty: boolean;
	/** The data types a new property can have. */
	datatypes: string[];
}

/** A column of a Table page. */
export interface TableColumn {
	/** `label`, `description`, `aliases`, `statements`, or `other` for what is not edited here. */
	kind: 'label' | 'description' | 'aliases' | 'statements' | 'other';
	/** The heading. */
	label: string;
	/** What is beneath it: a language, a property's ID. */
	sub: string;
	/** A term's language. */
	language?: string;
	/** A statements column's property. */
	property?: string;
	/** That property's data type. */
	datatype?: string;
}

/** A cell of a Table page. */
export interface TableCell {
	/** What it shows, in the form it is edited in. */
	text: string;
	/** What is shown beside the text: a label, a fold. */
	note: string;
	/** Whether it can be edited in the grid. */
	editable: boolean;
	/** The one best statement's ID, where the cell shows one. */
	guid?: string;
}

/** A row of a Table page. */
export interface TableRow {
	/** The entity's ID. */
	id: string;
	/** Its page. */
	href: string;
	/** The revision the cells were read at. */
	lastrevid: number;
	/** Whether the entity is there to be read. */
	missing: boolean;
	/** The cells, one per column. */
	cells: TableCell[];
}

/** A Table page the viewer may edit. */
export interface TableData extends Common {
	page: 'table';
	/** The page's title. */
	title: string;
	/** Its page ID, for the change tag. */
	pageId: number;
	/** The revision the definition was read at. */
	revid: number;
	/** The definition, as stored. */
	definition: string;
	/** Whether the definition has a default reference a new statement can carry. */
	hasReference: boolean;
	/** The columns. */
	columns: TableColumn[];
	/** The rows. */
	rows: TableRow[];
}

/** Either page's block. */
export type SheetData = CreateData | TableData;

/**
 * Reads the data block, if the page has one.
 *
 * @param doc The document
 * @return The data, or null
 */
export function readSheetData( doc: Document ): SheetData | null {
	const el = doc.getElementById( 'ts-sheet-data' );
	if ( !el || !el.textContent ) {
		return null;
	}
	try {
		return JSON.parse( el.textContent ) as SheetData;
	} catch {
		return null;
	}
}

/**
 * A message with `$1`, `$2`, … replaced; `⧼key⧽` when it is missing.
 *
 * @param data The block
 * @param key The message key
 * @param params The parameters
 * @return The message
 */
export function msg( data: Common, key: string, ...params: string[] ): string {
	const raw = data.messages[ key ];
	if ( raw === undefined ) {
		return `⧼${ key }⧽`;
	}
	return raw.replace( /\$(\d+)/g, ( whole, n: string ) => {
		const p = params[ Number( n ) - 1 ];
		return p === undefined ? whole : p;
	} );
}
