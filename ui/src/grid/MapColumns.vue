<template>
	<section class="ts-sheet-map" :aria-label="t( 'ts-sheet-map-title' )">
		<h2>{{ t( 'ts-sheet-map-title' ) }}</h2>
		<p>{{ intro }}</p>
		<table class="cdx-table__table ts-sheet-map__table">
			<thead>
				<tr>
					<th scope="col">
						{{ t( 'ts-sheet-map-column' ) }}
					</th>
					<th scope="col">
						{{ t( 'ts-sheet-map-sample' ) }}
					</th>
					<th scope="col">
						{{ t( 'ts-sheet-map-becomes' ) }}
					</th>
				</tr>
			</thead>
			<tbody>
				<tr v-for="( h, i ) in sheet.headers" :key="i">
					<th scope="row">
						{{ h || t( 'ts-sheet-map-unnamed', String( i + 1 ) ) }}
					</th>
					<td class="ts-sheet-map__sample">
						{{ samples[ i ] }}
					</td>
					<td>
						<select
							v-model="mappings[ i ].target"
							class="cdx-select"
							:aria-label="t( 'ts-sheet-map-becomes-for', h )">
							<option
								v-for="o in targets"
								:key="o"
								:value="o">
								{{ t( `ts-sheet-target-${ o }` ) }}
							</option>
						</select>
						<input
							v-if="isTerm( mappings[ i ].target )"
							v-model="mappings[ i ].language"
							class="cdx-text-input__input ts-sheet-map__short"
							type="text"
							:aria-label="t( 'ts-sheet-language' )">
						<template v-if="mappings[ i ].target === 'reference'">
							<EntityLookup
								:api="api"
								:lang="data.lang"
								type="property"
								:label="t( 'ts-sheet-reference-of' )"
								:placeholder="t( 'ts-sheet-reference-of' )"
								:initial-id="mappings[ i ].of"
								@pick="( id ) => { mappings[ i ].of = id; }"
							/>
						</template>
						<EntityLookup
							v-if="needsProperty( mappings[ i ].target )"
							:key="`${ i }-${ mappings[ i ].target }`"
							:api="api"
							:lang="data.lang"
							type="property"
							:label="t( 'ts-sheet-property' )"
							:placeholder="t( 'ts-sheet-property' )"
							:initial-id="mappings[ i ].property"
							:initial-label="mappings[ i ].propertyLabel"
							@pick="( id, label ) => pickProperty( i, id, label )"
						/>
						<template v-if="mappings[ i ].target === 'new'">
							<input
								v-model="mappings[ i ].label"
								class="cdx-text-input__input"
								type="text"
								:aria-label="t( 'ts-sheet-new-label' )"
								:placeholder="t( 'ts-sheet-new-label' )">
							<select
								v-model="mappings[ i ].datatype"
								class="cdx-select"
								:aria-label="t( 'ts-sheet-datatype' )">
								<option value="">
									{{ t( 'ts-sheet-datatype-later' ) }}
								</option>
								<option
									v-for="d in data.datatypes"
									:key="d"
									:value="d">
									{{ d }}
								</option>
							</select>
						</template>
					</td>
				</tr>
			</tbody>
		</table>
		<CdxMessage v-if="problem" type="error">
			{{ problem }}
		</CdxMessage>
		<div class="ts-sheet__actions">
			<CdxButton
				type="button"
				action="progressive"
				weight="primary"
				@click="done">
				{{ t( 'ts-sheet-map-apply' ) }}
			</CdxButton>
			<CdxButton type="button" @click="emit( 'cancel' )">
				{{ t( 'ts-sheet-cancel' ) }}
			</CdxButton>
		</div>
	</section>
</template>

<script setup lang="ts">
// The mapping step of an uploaded file (ADR 0106 §7; 21 §6.8): each source column with a
// sample of its values, and what it becomes in the table. What is evident is suggested:
// a header in the text's conventions, a property's ID, a property's label.
import { onMounted, reactive, ref } from 'vue';
import { CdxButton, CdxMessage } from '@wikimedia/codex';
import EntityLookup from '../edit/EntityLookup.vue';
import type { Api } from '../edit/api';
import { msg } from './data';
import type { CreateData } from './data';
import { apply, complete, suggest } from './mapping';
import type { Mapping, Target } from './mapping';
import type { Sheet } from './readers';

const props = defineProps<{
	data: CreateData;
	api: Api;
	sheet: Sheet;
}>();

const emit = defineEmits<{
	apply: [ rows: string[][], reference: string[] ];
	cancel: [];
}>();

const t = ( key: string, ...params: string[] ): string => msg( props.data, key, ...params );

const targets: Target[] = [ 'ignore', 'label', 'description', 'aliases', 'property', 'reads', 'reference', 'id' ];
if ( props.data.canCreateProperty ) {
	targets.splice( 5, 0, 'new' );
}

const mappings = reactive<Mapping[]>( props.sheet.headers.map( ( h ) => {
	const m = suggest( h, props.data.lang );
	// A new property is offered only to those who may create one.
	return m.target === 'new' && !props.data.canCreateProperty ? { ...m, target: 'ignore' as Target } : m;
} ) );
const problem = ref( '' );
const intro = t( 'ts-sheet-map-intro', String( props.sheet.rows.length ), String( props.sheet.headers.length ) );

/** A property the search found. */
interface Hit {
	id: string;
	display?: { label?: { value: string } };
}

function pickProperty( i: number, id: string, label: string ): void {
	mappings[ i ].property = id;
	mappings[ i ].propertyLabel = label;
}

const samples = props.sheet.headers.map( ( _, i ) => props.sheet.rows
	.map( ( r ) => r[ i ] ?? '' )
	.filter( ( v ) => v !== '' )
	.slice( 0, 3 )
	.join( ' · ' ) );

const isTerm = ( target: Target ): boolean => target === 'label' || target === 'description' || target === 'aliases';
const needsProperty = ( target: Target ): boolean => target === 'property' || target === 'reads' || target === 'reference';

// A header that is a property's label, exactly, is that property.
onMounted( async () => {
	await Promise.all( props.sheet.headers.map( async ( h, i ) => {
		if ( mappings[ i ].target !== 'ignore' || h.trim() === '' ) {
			return;
		}
		try {
			const body = await props.api.get( {
				action: 'wbsearchentities', search: h.trim(), language: props.data.lang, type: 'property', limit: 5
			} );
			const hits = ( body.search ?? [] ) as Hit[];
			const hit = hits.find( ( x ) => ( x.display?.label?.value ?? '' ).toLowerCase() === h.trim().toLowerCase() );
			if ( hit && mappings[ i ].target === 'ignore' ) {
				mappings[ i ].target = 'property';
				mappings[ i ].property = hit.id;
				mappings[ i ].propertyLabel = hit.display?.label?.value ?? hit.id;
			}
		} catch {
			// No suggestion.
		}
	} ) );
} );

function done(): void {
	const at = mappings.findIndex( ( m ) => !complete( m ) );
	if ( at >= 0 ) {
		problem.value = t( 'ts-sheet-map-incomplete', props.sheet.headers[ at ] || String( at + 1 ) );
		return;
	}
	if ( mappings.every( ( m ) => m.target === 'ignore' ) ) {
		problem.value = t( 'ts-sheet-map-nothing' );
		return;
	}
	const out = apply( props.sheet.headers, props.sheet.rows, mappings, props.data.lang );
	emit( 'apply', out.rows, out.reference );
}
</script>
