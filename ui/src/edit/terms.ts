/**
 * An entity's terms in one language, as the term editor holds them, and the
 * `wbeditentity` data that writes them in one revision.
 */

/** A label, a description and aliases in one language; empty where there is none. */
export interface Terms {
	label: string;
	description: string;
	aliases: string[];
}

interface Term {
	language: string;
	value: string;
}

/**
 * The terms of a `wbgetentities` entity in a language.
 *
 * @param entity The entity's JSON
 * @param lang The language
 * @return Its terms
 */
export function termsOf( entity: Record<string, unknown>, lang: string ): Terms {
	const one = ( key: string ): string => {
		const group = entity[ key ] as Record<string, Term> | undefined;
		return group?.[ lang ]?.value ?? '';
	};
	const aliases = entity.aliases as Record<string, Term[]> | undefined;
	return {
		label: one( 'labels' ),
		description: one( 'descriptions' ),
		aliases: ( aliases?.[ lang ] ?? [] ).map( ( a ) => a.value ),
	};
}

/**
 * The `data` of a `wbeditentity` that sets the terms of one language: an empty label or
 * description removes it, and the aliases replace the language's aliases (an empty list
 * removes them all).
 *
 * @param lang The language
 * @param terms The terms as they should be
 * @return The data
 */
export function termsEdit( lang: string, terms: Terms ): Record<string, unknown> {
	const aliases = terms.aliases.length > 0 ?
		terms.aliases.map( ( value ) => ( { language: lang, value } ) ) :
		[ { language: lang, value: '' } ];
	return {
		labels: { [ lang ]: { language: lang, value: terms.label } },
		descriptions: { [ lang ]: { language: lang, value: terms.description } },
		aliases: { [ lang ]: aliases },
	};
}
