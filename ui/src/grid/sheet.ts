/**
 * What the sheet component is given (ADR 0106 §6; 0045 §6): its headings and its rows.
 */

/** What names a thing in words: a row, a column. */
export type Words = ( name: string ) => string;

/** What names a cell in words, from its column's heading and its row. */
export type Words2 = ( heading: string, lead: string ) => string;

/** A column's heading. */
export interface SheetHeader {
	label: string;
	sub?: string;
	menu?: boolean;
}

/** A row. */
export interface SheetRow {
	key: string;
	/** The leading cell's text: a row number, an entity's ID. */
	lead: string;
	href?: string;
	/** The row's status, in words, and its kind as a class suffix. */
	status?: string;
	kind?: string;
	problems?: string[];
	removable?: boolean;
	cells: string[];
	readonly?: boolean[];
	changed?: boolean[];
	notes?: string[];
}
