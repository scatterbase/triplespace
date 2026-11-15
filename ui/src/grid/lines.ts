/**
 * The lines of the statements on every item and of the reference on every statement
 * (21 §6.6): one `P31:Q5` each, a reference's value either given or the header of a
 * column in braces, `P854:{source url}`. As the page reads them
 * (`create_entities/table.rs`, `lines_of`).
 */

/** One line: a property and its value, or the column the value is read from. */
export interface Line {
	/** The property's ID, or nothing yet. */
	property: string;
	/** The value as written; empty when a column is read. */
	value: string;
	/** The header of the column the value is read from, or nothing. */
	column: string;
}

/**
 * Reads the lines of a text box. A line that is not `property:value` is shown as a line
 * with no property, its text as the value, for the editor to give it one.
 *
 * @param text The box's text
 * @return The lines
 */
export function linesOf( text: string ): Line[] {
	return text.split( '\n' ).map( ( l ) => l.trim() ).filter( ( l ) => l !== '' ).map( ( raw ) => {
		const at = raw.indexOf( ':' );
		const property = at < 0 ? '' : raw.slice( 0, at ).trim().toUpperCase();
		const value = at < 0 ? '' : raw.slice( at + 1 ).trim();
		if ( !/^[A-Z]{0,3}P\d+$/.test( property ) || value === '' ) {
			return { property: '', value: raw, column: '' };
		}
		const read = /^\{(.*)\}$/.exec( value );
		return read && read[ 1 ].trim() !== '' ?
			{ property, value: '', column: read[ 1 ].trim() } :
			{ property, value, column: '' };
	} );
}

/**
 * Writes the lines as the text box holds them. A line with no property or no value yet
 * is left out.
 *
 * @param lines The lines
 * @return The text
 */
export function textOfLines( lines: Line[] ): string {
	return lines
		.filter( ( l ) => l.property !== '' && ( l.column !== '' || l.value.trim() !== '' ) )
		.map( ( l ) => `${ l.property }:${ l.column !== '' ? `{${ l.column }}` : l.value.trim() }` )
		.join( '\n' );
}
